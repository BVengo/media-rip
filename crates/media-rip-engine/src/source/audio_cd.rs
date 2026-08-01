use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_rip_core::{
    CoverArt, MediaEngineError, MediaTag, StreamKind, Title, Track, ports::CancellationToken,
    util::sanitize_file_name,
};
use serde::Deserialize;
use sha1::{Digest, Sha1};

use crate::process::ProcessSupervisor;

const CD_SECTORS_PER_SECOND: u64 = 75;
const CD_BYTES_PER_SECTOR: u64 = 2_352;
const MUSICBRAINZ_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
const MAX_COVER_ART_BYTES: u64 = 10 * 1024 * 1024;
static LAST_MUSICBRAINZ_REQUEST: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioCdCatalog {
    pub album: Option<String>,
    pub artist: Option<String>,
    pub titles: Vec<Title>,
    pub musicbrainz_error: Option<String>,
    pub exact_musicbrainz_match: bool,
    pub cover_art: Option<CoverArt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TocTrack {
    number: u32,
    sectors: u64,
    start_sector: u64,
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
    if auto_tagging && !cancellation.is_cancelled() {
        match musicbrainz_lookup(&toc, &cd_text, cancellation) {
            Ok((metadata, exact_match)) => {
                apply_musicbrainz_metadata(&mut titles, &metadata);
                album = Some(metadata.album.clone());
                album_artist = nonempty(metadata.album_artist.clone()).or(album_artist);
                exact_musicbrainz_match = exact_match;
                cover_art = fetch_cover_art(&metadata.release_id, cancellation).ok();
            }
            Err(error) => musicbrainz_error = Some(error),
        }
    }

    Ok(AudioCdCatalog {
        album,
        artist: album_artist,
        titles,
        musicbrainz_error,
        exact_musicbrainz_match,
        cover_art,
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

#[derive(Debug, Deserialize)]
struct DiscLookup {
    releases: Vec<MbRelease>,
}

#[derive(Debug, Deserialize)]
struct ReleaseSearch {
    #[serde(default)]
    releases: Vec<ReleaseSearchResult>,
}

#[derive(Debug, Deserialize)]
struct ReleaseSearchResult {
    id: String,
    #[serde(default)]
    score: u32,
    #[serde(rename = "track-count", default)]
    track_count: usize,
}

#[derive(Debug, Deserialize)]
struct MbRelease {
    id: String,
    title: String,
    date: Option<String>,
    country: Option<String>,
    barcode: Option<String>,
    status: Option<String>,
    quality: Option<String>,
    #[serde(rename = "text-representation")]
    text_representation: Option<MbTextRepresentation>,
    #[serde(rename = "label-info", default)]
    label_info: Vec<MbLabelInfo>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    media: Vec<MbMedium>,
    #[serde(rename = "release-group")]
    release_group: Option<MbReleaseGroup>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroup {
    id: String,
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(rename = "secondary-types", default)]
    secondary_types: Vec<String>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbArtistCredit {
    name: String,
    #[serde(default)]
    joinphrase: String,
    artist: Option<MbArtist>,
}

#[derive(Debug, Deserialize)]
struct MbArtist {
    id: String,
    name: String,
    #[serde(rename = "sort-name")]
    sort_name: String,
}

#[derive(Debug, Deserialize)]
struct MbTextRepresentation {
    script: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbLabelInfo {
    #[serde(rename = "catalog-number")]
    catalog_number: Option<String>,
    label: Option<MbLabel>,
}

#[derive(Debug, Deserialize)]
struct MbLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MbMedium {
    position: u32,
    format: Option<String>,
    #[serde(default)]
    discs: Vec<MbDisc>,
    #[serde(default)]
    tracks: Vec<MbTrack>,
}

#[derive(Debug, Deserialize)]
struct MbDisc {
    id: String,
}

#[derive(Debug, Deserialize)]
struct MbTrack {
    id: String,
    position: u32,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<MbArtistCredit>,
    length: Option<u64>,
    recording: MbRecording,
}

#[derive(Debug, Deserialize)]
struct MbRecording {
    id: String,
}

#[derive(Debug)]
struct MatchedDisc {
    disc_id: String,
    album: String,
    album_artist: String,
    album_artist_sort: String,
    album_artist_ids: String,
    release_id: String,
    release_group_id: Option<String>,
    release_type: Option<String>,
    release_status: Option<String>,
    date: Option<String>,
    original_date: Option<String>,
    country: Option<String>,
    barcode: Option<String>,
    catalog_number: Option<String>,
    label: Option<String>,
    script: Option<String>,
    disc_number: u32,
    disc_total: u32,
    media_format: Option<String>,
    tracks: Vec<MatchedTrack>,
}

#[derive(Debug)]
struct MatchedTrack {
    position: u32,
    title: String,
    artist: String,
    artists: String,
    artist_sort: String,
    artist_ids: String,
    recording_id: String,
    release_track_id: String,
}

fn musicbrainz_lookup(
    toc: &[TocTrack],
    cd_text: &CdText,
    cancellation: &CancellationToken,
) -> Result<(MatchedDisc, bool), String> {
    let disc_id = musicbrainz_disc_id(toc).ok_or_else(|| "invalid Audio CD TOC".to_owned())?;
    let url = musicbrainz_url(&disc_id);
    wait_for_musicbrainz_slot(cancellation)?;
    let exact_result = musicbrainz_request(ureq::get(&url)).and_then(|response| {
        match_musicbrainz_response(response, &disc_id, preferred_release_country().as_deref())
    });
    match exact_result {
        Ok(metadata) => Ok((metadata, true)),
        Err(exact_error) => musicbrainz_release_fallback(toc, cd_text, &disc_id, cancellation)
            .map(|metadata| (metadata, false))
            .map_err(|fallback_error| {
                format!("{exact_error}; release fallback failed: {fallback_error}")
            }),
    }
}

fn musicbrainz_request<T: for<'de> Deserialize<'de>>(request: ureq::Request) -> Result<T, String> {
    request
        .set(
            "User-Agent",
            concat!(
                "MediaRip/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/BVengo/media-rip)"
            ),
        )
        .set("Accept", "application/json")
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|error| error.to_string())?
        .into_json()
        .map_err(|error| error.to_string())
}

fn musicbrainz_release_fallback(
    toc: &[TocTrack],
    cd_text: &CdText,
    disc_id: &str,
    cancellation: &CancellationToken,
) -> Result<MatchedDisc, String> {
    let album = cd_text
        .album
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CD-TEXT has no album title".to_owned())?;
    let artist = cd_text
        .artist
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CD-TEXT has no album artist".to_owned())?;
    let query = format!(
        "release:\"{}\" AND artist:\"{}\" AND tracks:{}",
        escape_musicbrainz_query(album),
        escape_musicbrainz_query(artist),
        toc.len()
    );
    wait_for_musicbrainz_slot(cancellation)?;
    let search: ReleaseSearch = musicbrainz_request(
        ureq::get("https://musicbrainz.org/ws/2/release/")
            .query("query", &query)
            .query("fmt", "json")
            .query("limit", "5"),
    )?;

    let mut best = None;
    for candidate in search
        .releases
        .into_iter()
        .filter(|candidate| candidate.score >= 80 && candidate.track_count == toc.len())
    {
        if cancellation.is_cancelled() {
            return Err("MusicBrainz lookup cancelled".to_owned());
        }
        wait_for_musicbrainz_slot(cancellation)?;
        let release: MbRelease = musicbrainz_request(ureq::get(&format!(
            "https://musicbrainz.org/ws/2/release/{}?inc=recordings+artist-credits+release-groups+labels&fmt=json",
            candidate.id
        )))?;
        let candidate_medium = release
            .media
            .iter()
            .enumerate()
            .filter_map(|(index, medium)| {
                fallback_medium_score(toc, cd_text, medium, candidate.score)
                    .map(|score| (score, index))
            })
            .max_by_key(|(score, _)| *score);
        if let Some((score, medium_index)) = candidate_medium
            && best
                .as_ref()
                .is_none_or(|(best_score, _, _)| score > *best_score)
        {
            best = Some((score, release, medium_index));
        }
    }

    let (_, release, medium_index) = best.ok_or_else(|| {
        "MusicBrainz returned no release matching the CD-TEXT track list".to_owned()
    })?;
    Ok(matched_musicbrainz_release(release, medium_index, disc_id))
}

fn escape_musicbrainz_query(value: &str) -> String {
    value.chars().fold(String::new(), |mut escaped, character| {
        if matches!(character, '\\' | '"') {
            escaped.push('\\');
        }
        escaped.push(character);
        escaped
    })
}

fn fallback_medium_score(
    toc: &[TocTrack],
    cd_text: &CdText,
    medium: &MbMedium,
    search_score: u32,
) -> Option<(u32, u32, std::cmp::Reverse<u64>)> {
    if medium.tracks.len() != toc.len() {
        return None;
    }
    let title_matches = medium
        .tracks
        .iter()
        .filter(|track| {
            cd_text
                .tracks
                .get(&track.position)
                .and_then(|text| text.title.as_deref())
                .is_some_and(|title| {
                    normalize_music_title(title) == normalize_music_title(&track.title)
                })
        })
        .count() as u32;
    let duration_error = medium
        .tracks
        .iter()
        .filter_map(|track| {
            let actual = toc
                .iter()
                .find(|toc_track| toc_track.number == track.position)?;
            let expected = track.length?;
            let actual = actual.sectors.saturating_mul(1_000) / CD_SECTORS_PER_SECOND;
            Some(actual.abs_diff(expected))
        })
        .sum::<u64>();
    let known_titles = cd_text
        .tracks
        .values()
        .filter(|track| track.title.is_some())
        .count() as u32;
    if known_titles > 0 && title_matches.saturating_mul(3) < known_titles.saturating_mul(2) {
        return None;
    }
    Some((
        title_matches,
        search_score,
        std::cmp::Reverse(duration_error),
    ))
}

fn normalize_music_title(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn musicbrainz_url(disc_id: &str) -> String {
    format!(
        "https://musicbrainz.org/ws/2/discid/{disc_id}?inc=recordings+artist-credits+release-groups+labels&cdstubs=no&fmt=json"
    )
}

fn cover_art_url(release_id: &str) -> String {
    format!("https://coverartarchive.org/release/{release_id}/front-500")
}

fn fetch_cover_art(release_id: &str, cancellation: &CancellationToken) -> Result<CoverArt, String> {
    if cancellation.is_cancelled() {
        return Err("cover artwork lookup cancelled".to_owned());
    }
    let response = ureq::get(&cover_art_url(release_id))
        .set(
            "User-Agent",
            concat!(
                "MediaRip/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/BVengo/media-rip)"
            ),
        )
        .set("Accept", "image/jpeg,image/png")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|error| error.to_string())?;
    let mime_type = response
        .header("Content-Type")
        .and_then(|value| value.split(';').next())
        .unwrap_or("image/jpeg")
        .to_owned();
    if !matches!(mime_type.as_str(), "image/jpeg" | "image/png") {
        return Err(format!("unsupported cover artwork type {mime_type}"));
    }
    let mut data = Vec::new();
    response
        .into_reader()
        .take(MAX_COVER_ART_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|error| error.to_string())?;
    if data.is_empty() {
        return Err("cover artwork response was empty".to_owned());
    }
    if data.len() as u64 > MAX_COVER_ART_BYTES {
        return Err("cover artwork exceeded the 10 MiB limit".to_owned());
    }
    Ok(CoverArt::new(mime_type, data))
}

fn wait_for_musicbrainz_slot(cancellation: &CancellationToken) -> Result<(), String> {
    let requests = LAST_MUSICBRAINZ_REQUEST.get_or_init(|| Mutex::new(None));
    let mut last_request = requests
        .lock()
        .map_err(|_| "MusicBrainz request limiter is unavailable".to_owned())?;
    while let Some(remaining) =
        last_request.and_then(|last| MUSICBRAINZ_REQUEST_INTERVAL.checked_sub(last.elapsed()))
    {
        if cancellation.is_cancelled() {
            return Err("MusicBrainz lookup cancelled".to_owned());
        }
        thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    *last_request = Some(Instant::now());
    Ok(())
}

fn match_musicbrainz_response(
    response: DiscLookup,
    disc_id: &str,
    preferred_country: Option<&str>,
) -> Result<MatchedDisc, String> {
    response
        .releases
        .into_iter()
        .enumerate()
        .filter_map(|(index, release)| {
            let disc_total = release.media.len() as u32;
            let medium_index = release
                .media
                .iter()
                .position(|medium| medium.discs.iter().any(|disc| disc.id == disc_id))?;
            let score = release_score(&release, preferred_country);
            let mut matched = matched_musicbrainz_release(release, medium_index, disc_id);
            matched.disc_total = disc_total;
            Some((score, std::cmp::Reverse(index), matched))
        })
        .max_by_key(|(score, index, _)| (*score, *index))
        .map(|(_, _, matched)| matched)
        .ok_or_else(|| "MusicBrainz returned no exact medium match".to_owned())
}

fn matched_musicbrainz_release(
    release: MbRelease,
    medium_index: usize,
    disc_id: &str,
) -> MatchedDisc {
    let medium = &release.media[medium_index];
    let album_artist = artist_credit(&release.artist_credit);
    let album_artist_sort = artist_credit_sort(&release.artist_credit);
    let album_artist_ids = artist_ids(&release.artist_credit);
    let catalog_number = joined_nonempty(
        release
            .label_info
            .iter()
            .filter_map(|info| info.catalog_number.as_deref()),
    );
    let label = joined_nonempty(
        release
            .label_info
            .iter()
            .filter_map(|info| info.label.as_ref().map(|label| label.name.as_str())),
    );
    let release_type = release.release_group.as_ref().and_then(release_types);
    let original_date = release
        .release_group
        .as_ref()
        .and_then(|group| group.first_release_date.clone());
    let tracks = medium
        .tracks
        .iter()
        .map(|track| MatchedTrack {
            position: track.position,
            title: track.title.clone(),
            artist: artist_credit(&track.artist_credit),
            artists: artist_names(&track.artist_credit),
            artist_sort: artist_credit_sort(&track.artist_credit),
            artist_ids: artist_ids(&track.artist_credit),
            recording_id: track.recording.id.clone(),
            release_track_id: track.id.clone(),
        })
        .collect();
    MatchedDisc {
        disc_id: disc_id.to_owned(),
        album: release.title,
        album_artist,
        album_artist_sort,
        album_artist_ids,
        release_id: release.id,
        release_group_id: release.release_group.as_ref().map(|group| group.id.clone()),
        release_type,
        release_status: release.status,
        date: release.date,
        original_date,
        country: release.country,
        barcode: release.barcode,
        catalog_number,
        label,
        script: release.text_representation.and_then(|text| text.script),
        disc_number: medium.position,
        disc_total: release.media.len() as u32,
        media_format: medium.format.clone(),
        tracks,
    }
}

fn preferred_release_country() -> Option<String> {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find_map(|locale| locale_country(&locale))
}

fn locale_country(locale: &str) -> Option<String> {
    let locale = locale.split(['.', '@', ':']).next()?;
    let (_, country) = locale.split_once(['_', '-'])?;
    (country.len() == 2
        && country
            .chars()
            .all(|character| character.is_ascii_alphabetic()))
    .then(|| country.to_ascii_uppercase())
}

fn release_score(release: &MbRelease, preferred_country: Option<&str>) -> (u8, u8, u8, u8) {
    let status = match release.status.as_deref() {
        Some("Official") => 6,
        Some("Promotion") => 5,
        Some("Bootleg") => 4,
        Some("Pseudo-Release") => 3,
        Some("Withdrawn") => 2,
        Some("Cancelled") => 1,
        _ => 0,
    };
    let country = match release.country.as_deref() {
        Some(value) if preferred_country == Some(value) => 2,
        Some("XW") => 1,
        _ => 0,
    };
    let quality = match release.quality.as_deref() {
        Some("high") => 2,
        Some("normal") => 1,
        _ => 0,
    };
    let completeness = [
        release.date.is_some(),
        release.country.is_some(),
        release.barcode.is_some(),
        !release.label_info.is_empty(),
        release.text_representation.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count() as u8;
    (status, country, quality, completeness)
}

fn release_types(group: &MbReleaseGroup) -> Option<String> {
    joined_nonempty(
        group
            .primary_type
            .iter()
            .chain(group.secondary_types.iter())
            .map(|value| value.to_ascii_lowercase()),
    )
}

fn musicbrainz_disc_id(toc: &[TocTrack]) -> Option<String> {
    let first = toc.first()?.number;
    let last = toc.last()?.number;
    if first == 0 || last > 99 || first > last {
        return None;
    }
    let leadout = toc.last()?.start_sector + toc.last()?.sectors + 150;
    let mut input = format!("{first:02X}{last:02X}{leadout:08X}");
    let offsets = toc
        .iter()
        .map(|track| (track.number, track.start_sector + 150))
        .collect::<HashMap<_, _>>();
    for number in 1..=99 {
        input.push_str(&format!(
            "{:08X}",
            offsets.get(&number).copied().unwrap_or(0)
        ));
    }
    let digest = Sha1::digest(input.as_bytes());
    Some(
        STANDARD
            .encode(digest)
            .replace('+', ".")
            .replace('/', "_")
            .replace('=', "-"),
    )
}

fn apply_musicbrainz_metadata(titles: &mut [Title], metadata: &MatchedDisc) {
    for title in titles {
        let number = title.source_title_id.parse::<u32>().unwrap_or_default();
        let Some(track) = metadata
            .tracks
            .iter()
            .find(|track| track.position == number)
        else {
            continue;
        };
        title.name.clone_from(&track.title);
        title.output_file = format!("{number:02} - {}.flac", sanitize_file_name(&track.title));
        if let Some(audio) = title.tracks.first_mut() {
            audio.name =
                nonempty(track.artist.clone()).or_else(|| nonempty(metadata.album_artist.clone()));
        }
        title.tags = vec![
            MediaTag::new("title", &track.title),
            MediaTag::new("album", &metadata.album),
            MediaTag::new(
                "artist",
                nonempty(track.artist.clone()).unwrap_or_else(|| metadata.album_artist.clone()),
            ),
            MediaTag::new("album_artist", &metadata.album_artist),
            MediaTag::new("artists", &track.artists),
            MediaTag::new("artistsort", &track.artist_sort),
            MediaTag::new("albumartistsort", &metadata.album_artist_sort),
            MediaTag::new("track", number.to_string()),
            MediaTag::new("totaltracks", metadata.tracks.len().to_string()),
            MediaTag::new("discnumber", metadata.disc_number.to_string()),
            MediaTag::new("totaldiscs", metadata.disc_total.to_string()),
            MediaTag::new("musicbrainz_trackid", &track.recording_id),
            MediaTag::new("musicbrainz_releasetrackid", &track.release_track_id),
            MediaTag::new("musicbrainz_artistid", &track.artist_ids),
            MediaTag::new("musicbrainz_albumartistid", &metadata.album_artist_ids),
            MediaTag::new("musicbrainz_albumid", &metadata.release_id),
            MediaTag::new("musicbrainz_discid", &metadata.disc_id),
        ];
        for (key, value) in [
            ("date", metadata.date.as_deref()),
            ("originaldate", metadata.original_date.as_deref()),
            (
                "originalyear",
                metadata
                    .original_date
                    .as_deref()
                    .and_then(|date| date.get(..4)),
            ),
            ("releasecountry", metadata.country.as_deref()),
            ("barcode", metadata.barcode.as_deref()),
            ("catalognumber", metadata.catalog_number.as_deref()),
            ("label", metadata.label.as_deref()),
            ("media", metadata.media_format.as_deref()),
            ("releasestatus", metadata.release_status.as_deref()),
            ("releasetype", metadata.release_type.as_deref()),
            ("script", metadata.script.as_deref()),
            (
                "musicbrainz_releasegroupid",
                metadata.release_group_id.as_deref(),
            ),
        ] {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                title.tags.push(MediaTag::new(key, value));
            }
        }
    }
}

fn artist_credit(credits: &[MbArtistCredit]) -> String {
    credits
        .iter()
        .map(|credit| format!("{}{}", credit.name, credit.joinphrase))
        .collect()
}

fn artist_credit_sort(credits: &[MbArtistCredit]) -> String {
    credits
        .iter()
        .map(|credit| {
            format!(
                "{}{}",
                credit
                    .artist
                    .as_ref()
                    .map(|artist| artist.sort_name.as_str())
                    .unwrap_or(&credit.name),
                credit.joinphrase
            )
        })
        .collect()
}

fn artist_names(credits: &[MbArtistCredit]) -> String {
    joined_nonempty(credits.iter().map(|credit| {
        credit
            .artist
            .as_ref()
            .map(|artist| artist.name.as_str())
            .unwrap_or(&credit.name)
    }))
    .unwrap_or_default()
}

fn artist_ids(credits: &[MbArtistCredit]) -> String {
    joined_nonempty(
        credits
            .iter()
            .filter_map(|credit| credit.artist.as_ref().map(|artist| artist.id.as_str())),
    )
    .unwrap_or_default()
}

fn joined_nonempty<I, S>(values: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let value = values
        .into_iter()
        .map(|value| value.as_ref().trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (!value.is_empty()).then_some(value)
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
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
    fn calculates_the_musicbrainz_disc_id_from_the_toc() {
        let offsets = [
            150_u64, 22_767, 41_887, 58_317, 72_102, 91_375, 104_652, 115_380, 132_165, 143_932,
            159_870, 174_597,
        ];
        let leadout = 267_257;
        let toc = offsets
            .iter()
            .enumerate()
            .map(|(index, offset)| TocTrack {
                number: index as u32 + 1,
                start_sector: offset - 150,
                sectors: offsets.get(index + 1).copied().unwrap_or(leadout) - offset,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            musicbrainz_disc_id(&toc).as_deref(),
            Some("I5l9cCSFccLKFEKS.7wqSZAorPU-")
        );
    }

    #[test]
    fn requests_only_valid_discid_includes() {
        let url = musicbrainz_url("disc-id");
        assert!(url.contains("inc=recordings+artist-credits+release-groups"));
        assert!(url.contains("+labels"));
        assert!(url.contains("cdstubs=no"));
        assert!(!url.contains("+media"));
        assert!(!url.contains("+discids"));
    }

    #[test]
    fn release_fallback_requires_a_matching_cd_text_track_list() {
        let release: MbRelease = serde_json::from_str(
            r#"{
                "id": "release-id",
                "title": "The Album",
                "artist-credit": [],
                "media": [{
                    "position": 1,
                    "format": "Digital Media",
                    "tracks": [
                        {
                            "id": "track-1",
                            "position": 1,
                            "title": "First Song",
                            "length": 100000,
                            "recording": {"id": "recording-1"}
                        },
                        {
                            "id": "track-2",
                            "position": 2,
                            "title": "Second Song",
                            "length": 200000,
                            "recording": {"id": "recording-2"}
                        },
                        {
                            "id": "track-3",
                            "position": 3,
                            "title": "Third Song",
                            "length": 300000,
                            "recording": {"id": "recording-3"}
                        }
                    ]
                }]
            }"#,
        )
        .unwrap();
        let toc = vec![
            TocTrack {
                number: 1,
                sectors: 7_500,
                start_sector: 0,
            },
            TocTrack {
                number: 2,
                sectors: 15_000,
                start_sector: 7_500,
            },
            TocTrack {
                number: 3,
                sectors: 22_500,
                start_sector: 22_500,
            },
        ];
        let mut cd_text = CdText::default();
        for (number, title) in [(1, "First Song"), (2, "Second Song"), (3, "Third Song")] {
            cd_text.tracks.insert(
                number,
                TrackText {
                    title: Some(title.into()),
                    artist: None,
                },
            );
        }

        assert_eq!(
            fallback_medium_score(&toc, &cd_text, &release.media[0], 100),
            Some((3, 100, std::cmp::Reverse(0)))
        );

        cd_text.tracks.get_mut(&1).unwrap().title = Some("Wrong".into());
        cd_text.tracks.get_mut(&2).unwrap().title = Some("Also Wrong".into());
        assert_eq!(
            fallback_medium_score(&toc, &cd_text, &release.media[0], 100),
            None
        );
    }

    #[test]
    fn escapes_musicbrainz_search_syntax_in_cd_text() {
        assert_eq!(
            escape_musicbrainz_query(r#"An "Album" \ Name"#),
            r#"An \"Album\" \\ Name"#
        );
    }

    #[test]
    fn cd_text_track_tag_is_a_single_number() {
        let tags = cd_text_tags("Song", Some("Album"), Some("Artist"), Some("Artist"), 7);
        assert!(tags.contains(&MediaTag::new("track", "7")));
    }

    #[test]
    fn parses_and_applies_an_exact_musicbrainz_release() {
        let response: DiscLookup = serde_json::from_str(
            r#"{
                "releases": [{
                    "id": "release-id",
                    "title": "The Album",
                    "date": "2024-03-02",
                    "country": "AU",
                    "barcode": "123456789",
                    "status": "Official",
                    "quality": "high",
                    "text-representation": {"language": "eng", "script": "Latn"},
                    "label-info": [{
                        "catalog-number": "ABC-123",
                        "label": {"id": "label-id", "name": "The Label"}
                    }],
                    "artist-credit": [{
                        "name": "The Album Artist",
                        "joinphrase": "",
                        "artist": {"id": "album-artist-id", "name": "The Album Artist", "sort-name": "Album Artist, The"}
                    }],
                    "release-group": {
                        "id": "group-id",
                        "primary-type": "Album",
                        "secondary-types": ["Compilation"],
                        "first-release-date": "1999-05-01"
                    },
                    "media": [{
                        "position": 1,
                        "format": "CD",
                        "discs": [{"id": "disc-id"}],
                        "tracks": [{
                            "id": "release-track-id",
                            "position": 1,
                            "title": "The Song",
                            "artist-credit": [{
                                "name": "The Track Artist",
                                "joinphrase": "",
                                "artist": {"id": "track-artist-id", "name": "The Track Artist", "sort-name": "Track Artist, The"}
                            }],
                            "recording": {"id": "recording-id"}
                        }]
                    }]
                }]
            }"#,
        )
        .unwrap();

        let matched = match_musicbrainz_response(response, "disc-id", Some("AU")).unwrap();
        let mut titles = vec![Title {
            id: 0,
            source_title_id: "1".into(),
            name: "Track 01".into(),
            is_main_title: false,
            duration_ms: 1_000,
            size_bytes: None,
            size_is_estimate: false,
            chapters: Vec::new(),
            tracks: vec![Track {
                index: 0,
                kind: StreamKind::Audio,
                codec: "PCM".into(),
                codec_long_name: None,
                language: None,
                name: None,
                properties: Vec::new(),
                selected: true,
            }],
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            output_file: "01 - Track 01.flac".into(),
            selected: true,
        }];

        apply_musicbrainz_metadata(&mut titles, &matched);

        assert_eq!(titles[0].name, "The Song");
        assert_eq!(
            titles[0].tracks[0].name.as_deref(),
            Some("The Track Artist")
        );
        assert!(
            titles[0]
                .tags
                .contains(&MediaTag::new("album", "The Album"))
        );
        assert!(
            titles[0]
                .tags
                .contains(&MediaTag::new("musicbrainz_discid", "disc-id"))
        );
        assert!(titles[0].tags.contains(&MediaTag::new("track", "1")));
        for expected in [
            MediaTag::new("artists", "The Track Artist"),
            MediaTag::new("artistsort", "Track Artist, The"),
            MediaTag::new("albumartistsort", "Album Artist, The"),
            MediaTag::new("catalognumber", "ABC-123"),
            MediaTag::new("discnumber", "1"),
            MediaTag::new("totaldiscs", "1"),
            MediaTag::new("totaltracks", "1"),
            MediaTag::new("musicbrainz_artistid", "track-artist-id"),
            MediaTag::new("musicbrainz_albumartistid", "album-artist-id"),
            MediaTag::new("musicbrainz_trackid", "recording-id"),
            MediaTag::new("musicbrainz_releasetrackid", "release-track-id"),
            MediaTag::new("originaldate", "1999-05-01"),
            MediaTag::new("originalyear", "1999"),
            MediaTag::new("label", "The Label"),
            MediaTag::new("releasecountry", "AU"),
            MediaTag::new("releasestatus", "Official"),
            MediaTag::new("releasetype", "album; compilation"),
            MediaTag::new("script", "Latn"),
        ] {
            assert!(titles[0].tags.contains(&expected), "missing {expected:?}");
        }
    }

    #[test]
    fn exact_disc_selection_prefers_official_local_release_and_does_not_require_track_count() {
        let response: DiscLookup = serde_json::from_str(
            r#"{
                "releases": [
                    {
                        "id": "pseudo-us",
                        "title": "Wrong Version",
                        "country": "US",
                        "status": "Pseudo-Release",
                        "artist-credit": [],
                        "media": [{"position": 1, "discs": [{"id": "disc-id"}], "tracks": []}]
                    },
                    {
                        "id": "official-gb",
                        "title": "Foreign Version",
                        "country": "GB",
                        "status": "Official",
                        "artist-credit": [],
                        "media": [{"position": 2, "discs": [{"id": "disc-id"}], "tracks": []}]
                    },
                    {
                        "id": "official-au",
                        "title": "Local Version",
                        "country": "AU",
                        "status": "Official",
                        "artist-credit": [],
                        "media": [{"position": 2, "discs": [{"id": "disc-id"}], "tracks": []}]
                    }
                ]
            }"#,
        )
        .unwrap();

        let matched = match_musicbrainz_response(response, "disc-id", Some("AU")).unwrap();

        assert_eq!(matched.album, "Local Version");
        assert_eq!(matched.disc_number, 2);
    }

    #[test]
    fn extracts_country_from_common_system_locale_forms() {
        assert_eq!(locale_country("en_AU.UTF-8").as_deref(), Some("AU"));
        assert_eq!(locale_country("pt-BR").as_deref(), Some("BR"));
        assert_eq!(locale_country("C"), None);
    }

}
