use std::{
    path::Path,
    process::{Command, Output},
};

use media_rip_core::{
    CoverArt, MediaEngineError, MediaTag, StreamKind, Title, Track, ports::CancellationToken,
    util::sanitize_file_name,
};

use crate::{diagnostics, process::run_cancellable};

const CD_SECTORS_PER_SECOND: u64 = 75;
const CD_BYTES_PER_SECTOR: u64 = 2_352;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioCdCatalog {
    pub album: Option<String>,
    pub artist: Option<String>,
    pub titles: Vec<Title>,
    pub musicbrainz_error: Option<String>,
    pub exact_musicbrainz_match: bool,
    pub cover_art: Option<CoverArt>,
    pub candidates: Vec<media_rip_core::AudioMetadataCandidate>,
}

use crate::metadata::musicbrainz::{
    apply_musicbrainz_metadata, audio_candidate, fetch_cover_art, musicbrainz_lookup,
};
use media_rip_core::model::audio::{CdText, TocTrack};

pub(crate) fn audio_cd_catalog(
    device: &Path,
    auto_tagging: bool,
    cancellation: &CancellationToken,
) -> Result<AudioCdCatalog, MediaEngineError> {
    let mut query = Command::new("cdparanoia");
    query.args(["-Q", "-d"]).arg(device);
    let output = run_cancellable(query, cancellation).map_err(|error| match error {
        MediaEngineError::Process(message) if message.contains("could not run") => {
            MediaEngineError::MissingTool(
                "cdparanoia (required for secure Audio CD extraction)".to_owned(),
            )
        }
        error => error,
    })?;
    if !output.status.success() {
        return Err(process_error(
            "cdparanoia could not read the Audio CD",
            &output,
        ));
    }
    diagnostics::text(
        "cdparanoia",
        &format!("complete TOC query for {}", device.display()),
        &format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    );
    let toc_text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let toc = parse_cdparanoia_toc(&toc_text);
    if toc.is_empty() {
        return Err(MediaEngineError::Process(
            "cdparanoia found no readable audio tracks on the disc".to_owned(),
        ));
    }

    let cd_text = match read_cd_text(device, cancellation) {
        Ok(metadata) => metadata,
        Err(MediaEngineError::Cancelled) => return Err(MediaEngineError::Cancelled),
        Err(_) => CdText::default(),
    };
    let width = toc
        .iter()
        .map(|track| track.number)
        .max()
        .unwrap_or_default()
        .to_string()
        .len()
        .max(2);
    let mut titles = toc
        .iter()
        .enumerate()
        .map(|(id, track)| {
            let metadata = cd_text.tracks.get(&track.number);
            let name = metadata
                .and_then(|metadata| metadata.title.clone())
                .unwrap_or_else(|| format!("Track {:0width$}", track.number));
            let artist = metadata
                .and_then(|metadata| metadata.artist.clone())
                .or_else(|| cd_text.artist.clone());
            let mut properties = vec![
                "stereo".to_owned(),
                "44.1 kHz".to_owned(),
                "16-bit".to_owned(),
            ];
            if let Some(artist) = &artist {
                properties.push(artist.clone());
            }
            Title {
                dvd: None,
                id: id as u32,
                source_title_id: track.number.to_string(),
                disc_title_number: None,
                name: name.clone(),
                is_main_title: false,
                duration_ms: track.sectors.saturating_mul(1_000) / CD_SECTORS_PER_SECOND,
                size_bytes: Some(track.sectors.saturating_mul(CD_BYTES_PER_SECTOR)),
                size_is_estimate: false,
                chapters: Vec::new(),
                tracks: vec![Track {
                    index: 0,
                    kind: StreamKind::Audio,
                    codec: "PCM".to_owned(),
                    codec_long_name: Some(
                        "Compact Disc Digital Audio (16-bit stereo PCM)".to_owned(),
                    ),
                    language: None,
                    name: artist.clone(),
                    properties,
                    selected: true,
                }],
                segment_count: None,
                segment_map: None,
                duplicate_key: None,
                tags: cd_text_tags(
                    &name,
                    cd_text.album.as_deref(),
                    artist.as_deref(),
                    cd_text.artist.as_deref(),
                    track.number,
                ),
                video_metadata: None,
                output_file: format!(
                    "{:0width$} - {}.flac",
                    track.number,
                    sanitize_file_name(&name)
                ),
                selected: true,
            }
        })
        .collect::<Vec<_>>();

    let mut album = cd_text.album.clone();
    let mut album_artist = cd_text.artist.clone();
    let mut musicbrainz_error = None;
    let mut exact_musicbrainz_match = false;
    let mut cover_art = None;
    let mut candidates = Vec::new();
    if auto_tagging && !cancellation.is_cancelled() {
        match musicbrainz_lookup(&toc, &cd_text, cancellation) {
            Ok((mut matches, exact_match)) if exact_match && matches.len() == 1 => {
                let metadata = matches.remove(0);
                apply_musicbrainz_metadata(&mut titles, &metadata);
                album = Some(metadata.album.clone());
                album_artist = nonempty(metadata.album_artist.clone()).or(album_artist);
                exact_musicbrainz_match = exact_match;
                cover_art = fetch_cover_art(&metadata.release_id, cancellation).ok();
            }
            Ok((matches, exact_match)) => {
                candidates = matches
                    .iter()
                    .map(|matched| audio_candidate(&titles, matched, exact_match))
                    .collect();
            }
            Err(error) => musicbrainz_error = Some(error),
        }
    }

    if cancellation.is_cancelled() {
        return Err(MediaEngineError::Cancelled);
    }
    Ok(AudioCdCatalog {
        album,
        artist: album_artist,
        titles,
        musicbrainz_error,
        exact_musicbrainz_match,
        cover_art,
        candidates,
    })
}

fn read_cd_text(
    device: &Path,
    cancellation: &CancellationToken,
) -> Result<CdText, MediaEngineError> {
    let mut command = Command::new("cd-info");
    command.args([
        "--no-header",
        "--no-device-info",
        "--no-disc-mode",
        "--no-cddb",
        "--no-cddb-cache",
    ]);
    command.arg(format!("--cdrom-device={}", device.to_string_lossy()));
    let output = run_cancellable(command, cancellation)?;
    if !output.status.success() {
        return Ok(CdText::default());
    }
    let output = String::from_utf8_lossy(&output.stdout);
    diagnostics::text(
        "cd-info",
        &format!("complete CD-TEXT response for {}", device.display()),
        &output,
    );
    Ok(parse_cd_text(&output))
}

fn process_error(context: &str, output: &Output) -> MediaEngineError {
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    MediaEngineError::Process(if detail.is_empty() {
        format!("{context} ({})", output.status)
    } else {
        format!("{context}: {detail}")
    })
}

fn parse_cdparanoia_toc(output: &str) -> Vec<TocTrack> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (number, rest) = line.split_once('.')?;
            let number = number.trim().parse::<u32>().ok()?;
            let fields = rest.split_whitespace().collect::<Vec<_>>();
            let sectors = fields.first()?.parse::<u64>().ok()?;
            let start_sector = fields.get(2)?.parse::<u64>().ok()?;
            (number > 0 && sectors > 0).then_some(TocTrack {
                number,
                sectors,
                start_sector,
            })
        })
        .collect()
}

fn cd_text_tags(
    title: &str,
    album: Option<&str>,
    artist: Option<&str>,
    album_artist: Option<&str>,
    track_number: u32,
) -> Vec<MediaTag> {
    let mut tags = vec![
        MediaTag::new("title", title),
        MediaTag::new("track", track_number.to_string()),
    ];
    for (key, value) in [
        ("album", album),
        ("artist", artist),
        ("album_artist", album_artist),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            tags.push(MediaTag::new(key, value));
        }
    }
    tags
}

fn parse_cd_text(output: &str) -> CdText {
    let mut result = CdText::default();
    let mut current_track = None;
    for line in output.lines().map(str::trim) {
        if line == "CD-TEXT for Disc:" {
            current_track = None;
        } else if let Some(number) = line
            .strip_prefix("CD-TEXT for Track")
            .and_then(|value| value.trim_end_matches(':').trim().parse::<u32>().ok())
        {
            current_track = Some(number);
        } else if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match (current_track, key.trim()) {
                (None, "TITLE") => result.album = Some(value.to_owned()),
                (None, "PERFORMER") => result.artist = Some(value.to_owned()),
                (Some(track), "TITLE") => {
                    result.tracks.entry(track).or_default().title = Some(value.to_owned())
                }
                (Some(track), "PERFORMER") => {
                    result.tracks.entry(track).or_default().artist = Some(value.to_owned())
                }
                _ => {}
            }
        }
    }
    result
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_cdparanoia_track_lengths() {
        let toc = parse_cdparanoia_toc(
            "  1.    21403 [04:45.28]        0 [00:00.00] OK\n 10. 47130 [10:28.30] 168918 [37:32.18] OK\nTOTAL 216048",
        );
        assert_eq!(
            toc,
            vec![
                TocTrack {
                    number: 1,
                    sectors: 21_403,
                    start_sector: 0,
                },
                TocTrack {
                    number: 10,
                    sectors: 47_130,
                    start_sector: 168_918,
                }
            ]
        );
    }
    #[test]
    fn parses_disc_and_track_cd_text() {
        let metadata = parse_cd_text(
            "CD-TEXT for Disc:\n TITLE: Realization\n PERFORMER: Pete Wood\nCD-TEXT for Track  1:\n TITLE: A Thief In Paradise\n PERFORMER: Pete Wood\n",
        );
        assert_eq!(metadata.album.as_deref(), Some("Realization"));
        assert_eq!(metadata.artist.as_deref(), Some("Pete Wood"));
        assert_eq!(
            metadata.tracks[&1].title.as_deref(),
            Some("A Thief In Paradise")
        );
    }
    #[test]
    fn cd_text_track_tag_is_a_single_number() {
        let tags = cd_text_tags("Song", Some("Album"), Some("Artist"), Some("Artist"), 7);
        assert!(tags.contains(&MediaTag::new("track", "7")));
    }
}
