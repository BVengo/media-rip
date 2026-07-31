use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use media_rip_core::{
    MediaEngineError, StreamKind, Title, Track, ports::CancellationToken, util::sanitize_file_name,
};

use crate::process::ProcessSupervisor;

const CD_SECTORS_PER_SECOND: u64 = 75;
const CD_BYTES_PER_SECTOR: u64 = 2_352;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioCdCatalog {
    pub album: Option<String>,
    pub artist: Option<String>,
    pub titles: Vec<Title>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TocTrack {
    number: u32,
    sectors: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct CdText {
    album: Option<String>,
    artist: Option<String>,
    tracks: HashMap<u32, TrackText>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TrackText {
    title: Option<String>,
    artist: Option<String>,
}

pub(crate) fn audio_cd_catalog(
    device: &Path,
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
    let titles = toc
        .into_iter()
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
                id: id as u32,
                source_title_id: track.number.to_string(),
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
                    name: artist,
                    properties,
                    selected: true,
                }],
                segment_count: None,
                segment_map: None,
                duplicate_key: None,
                output_file: format!(
                    "{:0width$} - {}.flac",
                    track.number,
                    sanitize_file_name(&name)
                ),
                selected: true,
            }
        })
        .collect();

    Ok(AudioCdCatalog {
        album: cd_text.album,
        artist: cd_text.artist,
        titles,
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
    Ok(parse_cd_text(&String::from_utf8_lossy(&output.stdout)))
}

fn run_cancellable(
    mut command: Command,
    cancellation: &CancellationToken,
) -> Result<Output, MediaEngineError> {
    if cancellation.is_cancelled() {
        return Err(MediaEngineError::Cancelled);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let program = command.get_program().to_string_lossy().into_owned();
    let mut process = ProcessSupervisor::spawn(&mut command)
        .map_err(|error| MediaEngineError::Process(format!("could not run {program}: {error}")))?;
    let stdout = process.take_stdout().ok_or_else(|| {
        MediaEngineError::Process(format!("{program} output pipe is unavailable"))
    })?;
    let stderr = process
        .take_stderr()
        .ok_or_else(|| MediaEngineError::Process(format!("{program} error pipe is unavailable")))?;
    let stdout_reader = thread::spawn(move || read_bytes(stdout));
    let stderr_reader = thread::spawn(move || read_bytes(stderr));
    loop {
        if cancellation.is_cancelled() {
            let _ = process.terminate();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(MediaEngineError::Cancelled);
        }
        if let Some(status) = process
            .try_wait()
            .map_err(|error| MediaEngineError::Process(format!("{program} wait failed: {error}")))?
        {
            return Ok(Output {
                status,
                stdout: stdout_reader.join().unwrap_or_default(),
                stderr: stderr_reader.join().unwrap_or_default(),
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn read_bytes(mut reader: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = reader.read_to_end(&mut bytes);
    bytes
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
            let sectors = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            (number > 0 && sectors > 0).then_some(TocTrack { number, sectors })
        })
        .collect()
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
                    sectors: 21_403
                },
                TocTrack {
                    number: 10,
                    sectors: 47_130
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
}
