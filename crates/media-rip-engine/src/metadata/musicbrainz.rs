use crate::diagnostics;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_rip_core::{
    CoverArt, MediaTag, Title,
    model::audio::{CdText, TocTrack},
    ports::CancellationToken,
    util::sanitize_file_name,
};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{
    collections::HashMap,
    io::Read,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
const CD_SECTORS_PER_SECOND: u64 = 75;
const MUSICBRAINZ_REQUEST_INTERVAL: Duration = Duration::from_secs(1);
const MAX_COVER_ART_BYTES: u64 = 10 * 1024 * 1024;
static LAST_MUSICBRAINZ_REQUEST: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

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
pub(crate) struct MatchedDisc {
    disc_id: String,
    pub(crate) album: String,
    pub(crate) album_artist: String,
    album_artist_sort: String,
    album_artist_ids: String,
    pub(crate) release_id: String,
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

pub(crate) fn musicbrainz_lookup(
    toc: &[TocTrack],
    cd_text: &CdText,
    cancellation: &CancellationToken,
) -> Result<(Vec<MatchedDisc>, bool), String> {
    let disc_id = musicbrainz_disc_id(toc).ok_or_else(|| "invalid Audio CD TOC".to_owned())?;
    let url = musicbrainz_url(&disc_id);
    wait_for_musicbrainz_slot(cancellation)?;
    let exact_result = musicbrainz_request(ureq::get(&url)).and_then(|response| {
        match_musicbrainz_response(
            response,
            &disc_id,
            preferred_release_country().as_deref(),
            toc,
        )
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
    let operation = request.url().to_owned();
    let value = request
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
        .into_json::<serde_json::Value>()
        .map_err(|error| error.to_string())?;
    diagnostics::from_value("MusicBrainz", &operation, "network", value)
}

fn musicbrainz_release_fallback(
    toc: &[TocTrack],
    cd_text: &CdText,
    disc_id: &str,
    cancellation: &CancellationToken,
) -> Result<Vec<MatchedDisc>, String> {
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
        "release:\"{}\" AND artist:\"{}\"",
        escape_musicbrainz_query(album),
        escape_musicbrainz_query(artist)
    );
    wait_for_musicbrainz_slot(cancellation)?;
    let search: ReleaseSearch = musicbrainz_request(
        ureq::get("https://musicbrainz.org/ws/2/release/")
            .query("query", &query)
            .query("fmt", "json")
            .query("limit", "5"),
    )?;

    let mut matches = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for candidate in search.releases {
        if !seen.insert(candidate.id.clone()) {
            continue;
        }
        wait_for_musicbrainz_slot(cancellation)?;
        let release: MbRelease = match musicbrainz_request(ureq::get(&format!(
            "https://musicbrainz.org/ws/2/release/{}?inc=recordings+artist-credits+release-groups+labels&fmt=json",
            candidate.id
        ))) {
            Ok(release) => release,
            Err(error) => {
                log::warn!("MusicBrainz candidate lookup failed: {error}");
                continue;
            }
        };
        if normalize_music_title(&release.title) != normalize_music_title(album)
            || normalize_music_title(&artist_credit(&release.artist_credit))
                != normalize_music_title(artist)
        {
            continue;
        }
        for (medium_index, medium) in release.media.iter().enumerate() {
            if let Some(score) = fallback_medium_score(toc, cd_text, medium) {
                matches.push((
                    score,
                    matched_musicbrainz_release(&release, medium_index, disc_id),
                ));
            }
        }
    }
    matches.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    if matches.is_empty() {
        return Err("No release agrees with the album, artist and physical track list. Edit the metadata manually or retry lookup.".to_owned());
    }
    Ok(matches.into_iter().map(|(_, matched)| matched).collect())
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
) -> Option<(usize, std::cmp::Reverse<u64>)> {
    if !valid_medium(toc, medium) {
        return None;
    }
    let mut title_matches = 0;
    let mut duration_error = 0u64;
    let mut known_durations = 0;
    for actual in toc {
        let track = medium
            .tracks
            .iter()
            .find(|track| track.position == actual.number)?;
        if let Some(title) = cd_text
            .tracks
            .get(&actual.number)
            .and_then(|text| text.title.as_deref())
            .filter(|title| !title.trim().is_empty())
        {
            if normalize_music_title(title) != normalize_music_title(&track.title) {
                return None;
            }
            title_matches += 1;
        }
        if let Some(expected) = track.length.filter(|length| *length > 0) {
            let actual_ms = actual.sectors.saturating_mul(1000) / CD_SECTORS_PER_SECOND;
            let error = actual_ms.abs_diff(expected);
            // CD offsets and rounding can account for seconds, not minutes.
            if error > 5000 {
                return None;
            }
            duration_error = duration_error.saturating_add(error);
            known_durations += 1;
        }
    }
    // Missing durations cannot look better than measured matching durations.
    if title_matches == 0 && known_durations != toc.len() {
        return None;
    }
    Some((
        known_durations + title_matches,
        std::cmp::Reverse(duration_error),
    ))
}

fn valid_medium(toc: &[TocTrack], medium: &MbMedium) -> bool {
    !toc.is_empty()
        && medium.tracks.len() == toc.len()
        && medium.position > 0
        && medium
            .format
            .as_deref()
            .is_none_or(|format| format.contains("CD"))
        && toc.iter().all(|actual| {
            medium
                .tracks
                .iter()
                .filter(|track| track.position == actual.number && !track.title.trim().is_empty())
                .count()
                == 1
        })
}

fn normalize_music_title(value: &str) -> String {
    media_rip_core::model::matching::text_key(value)
}

fn musicbrainz_url(disc_id: &str) -> String {
    format!(
        "https://musicbrainz.org/ws/2/discid/{disc_id}?inc=recordings+artist-credits+release-groups+labels&cdstubs=no&fmt=json"
    )
}

fn cover_art_url(release_id: &str) -> String {
    format!("https://coverartarchive.org/release/{release_id}/front-500")
}

pub(crate) fn fetch_cover_art(
    release_id: &str,
    cancellation: &CancellationToken,
) -> Result<CoverArt, String> {
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
    if cancellation.is_cancelled() {
        return Err("MusicBrainz lookup cancelled".to_owned());
    }
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
    toc: &[TocTrack],
) -> Result<Vec<MatchedDisc>, String> {
    let mut matches = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for release in response.releases {
        for (index, medium) in release.media.iter().enumerate() {
            if medium.discs.iter().any(|disc| disc.id == disc_id)
                && valid_medium(toc, medium)
                && seen.insert((release.id.clone(), medium.position))
            {
                matches.push((
                    release_score(&release, preferred_country),
                    matched_musicbrainz_release(&release, index, disc_id),
                ));
            }
        }
    }
    matches.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    if matches.is_empty() {
        return Err("MusicBrainz returned no complete exact medium match".to_owned());
    }
    Ok(matches.into_iter().map(|(_, matched)| matched).collect())
}

pub(crate) fn audio_candidate(
    titles: &[Title],
    matched: &MatchedDisc,
    exact: bool,
) -> media_rip_core::AudioMetadataCandidate {
    let mut proposed = titles.to_vec();
    apply_musicbrainz_metadata(&mut proposed, matched);
    media_rip_core::AudioMetadataCandidate {
        id: format!("{}:{}", matched.release_id, matched.disc_number),
        album: matched.album.clone(),
        artist: matched.album_artist.clone(),
        edition: format!("{} · {} · disc {}/{} · {}", matched.date.as_deref().unwrap_or("Unknown date"), matched.country.as_deref().unwrap_or("Unknown country"), matched.disc_number, matched.disc_total, matched.catalog_number.as_deref().unwrap_or(&matched.release_id)),
        evidence: if exact { "The disc identifier matches more than one edition. Choose the edition printed on your packaging." } else { "The album, artist and track list are compatible, but no exact disc identifier matched. Confirm this release before applying it." }.to_owned(),
        tracks: proposed.into_iter().filter_map(|title| Some((title.source_title_id.parse().ok()?, title.tags))).collect(),
    }
}

fn matched_musicbrainz_release(
    release: &MbRelease,
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
        album: release.title.clone(),
        album_artist,
        album_artist_sort,
        album_artist_ids,
        release_id: release.id.clone(),
        release_group_id: release.release_group.as_ref().map(|group| group.id.clone()),
        release_type,
        release_status: release.status.clone(),
        date: release.date.clone(),
        original_date,
        country: release.country.clone(),
        barcode: release.barcode.clone(),
        catalog_number,
        label,
        script: release
            .text_representation
            .as_ref()
            .and_then(|text| text.script.clone()),
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
    if toc.iter().any(|track| track.sectors == 0)
        || toc.windows(2).any(|pair| {
            pair[0].number.checked_add(1) != Some(pair[1].number)
                || pair[0].start_sector.checked_add(pair[0].sectors) != Some(pair[1].start_sector)
        })
    {
        return None;
    }
    let leadout = toc
        .last()?
        .start_sector
        .checked_add(toc.last()?.sectors)?
        .checked_add(150)?;
    u32::try_from(leadout).ok()?;
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

pub(crate) fn apply_musicbrainz_metadata(titles: &mut [Title], metadata: &MatchedDisc) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use media_rip_core::{StreamKind, Track, model::audio::TrackText};
    fn fixture() -> (Vec<TocTrack>, CdText, MbRelease) {
        let toc = vec![
            TocTrack {
                number: 1,
                sectors: 7500,
                start_sector: 0,
            },
            TocTrack {
                number: 2,
                sectors: 15000,
                start_sector: 7500,
            },
        ];
        let text = CdText {
            album: Some("Book".into()),
            artist: Some("Author".into()),
            ..Default::default()
        };
        let release = serde_json::from_value(serde_json::json!({"id": "edition", "title": "Book", "artist-credit": [{"name": "Author"}], "media": [{"position": 1, "format": "CD", "discs": [{"id": "disc"}], "tracks": [
            {"id": "t1", "position": 1, "title": "Chapter 1", "length": 100000, "recording": {"id": "r1"}},
            {"id": "t2", "position": 2, "title": "Chapter 2", "length": 200000, "recording": {"id": "r2"}}
        ]}]})).unwrap();
        (toc, text, release)
    }

    #[test]
    fn missing_durations_are_not_zero_error_evidence() {
        let (toc, text, mut release) = fixture();
        for track in &mut release.media[0].tracks {
            track.length = None;
        }
        assert!(fallback_medium_score(&toc, &text, &release.media[0]).is_none());
    }

    #[test]
    fn structural_contradictions_defeat_matching_track_names() {
        let (toc, mut text, mut release) = fixture();
        for track in &release.media[0].tracks {
            text.tracks.insert(
                track.position,
                TrackText {
                    title: Some(track.title.clone()),
                    artist: None,
                },
            );
        }
        release.media[0].tracks[0].length = Some(600000);
        assert!(fallback_medium_score(&toc, &text, &release.media[0]).is_none());
        release.media[0].tracks[0].length = Some(100000);
        release.media[0].tracks[1].position = 1;
        assert!(fallback_medium_score(&toc, &text, &release.media[0]).is_none());
    }

    #[test]
    fn small_timing_differences_and_missing_year_are_acceptable_for_review() {
        let (toc, text, mut release) = fixture();
        release.media[0].tracks[0].length = Some(102000);
        assert!(release.date.is_none());
        assert!(fallback_medium_score(&toc, &text, &release.media[0]).is_some());
    }

    #[test]
    fn exact_identifier_keeps_editions_ambiguous_and_deduplicates_responses() {
        let (toc, _, first) = fixture();
        let (_, _, mut second) = fixture();
        second.id = "reissue".into();
        second.date = Some("2025".into());
        let (_, _, duplicate) = fixture();
        let matches = match_musicbrainz_response(
            DiscLookup {
                releases: vec![first, second, duplicate],
            },
            "disc",
            Some("AU"),
            &toc,
        )
        .unwrap();
        assert_eq!(matches.len(), 2);
        let (_, _, wrong) = fixture();
        assert!(
            match_musicbrainz_response(
                DiscLookup {
                    releases: vec![wrong]
                },
                "different",
                None,
                &toc
            )
            .is_err()
        );
    }

    #[test]
    fn multi_disc_box_set_matches_the_medium_not_the_release_track_total() {
        let (toc, text, mut release) = fixture();
        let (_, _, mut other) = fixture();
        other.media[0].position = 2;
        other.media[0].discs.clear();
        release.media.extend(other.media);
        assert!(fallback_medium_score(&toc, &text, &release.media[0]).is_some());
        let matches = match_musicbrainz_response(
            DiscLookup {
                releases: vec![release],
            },
            "disc",
            None,
            &toc,
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].disc_total, 2);
    }

    #[test]
    fn incompatible_medium_and_abridged_track_list_are_rejected() {
        let (toc, text, mut release) = fixture();
        release.media[0].format = Some("Digital Media".into());
        assert!(fallback_medium_score(&toc, &text, &release.media[0]).is_none());
        release.media[0].format = Some("CD".into());
        release.media[0].tracks.pop();
        assert!(fallback_medium_score(&toc, &text, &release.media[0]).is_none());
    }

    #[test]
    fn malformed_toc_cannot_overflow_or_generate_a_trusted_identifier() {
        let (mut toc, _, _) = fixture();
        toc[0].sectors = u64::MAX;
        assert!(musicbrainz_disc_id(&toc).is_none());
        let (mut toc, _, _) = fixture();
        toc[1].number = 1;
        assert!(musicbrainz_disc_id(&toc).is_none());
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
                    "format": "CD",
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
            fallback_medium_score(&toc, &cd_text, &release.media[0]),
            Some((6, std::cmp::Reverse(0)))
        );

        cd_text.tracks.get_mut(&1).unwrap().title = Some("Wrong".into());
        cd_text.tracks.get_mut(&2).unwrap().title = Some("Also Wrong".into());
        assert_eq!(
            fallback_medium_score(&toc, &cd_text, &release.media[0]),
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

        let matched = match_musicbrainz_response(
            response,
            "disc-id",
            Some("AU"),
            &[TocTrack {
                number: 1,
                sectors: 75,
                start_sector: 0,
            }],
        )
        .unwrap()
        .remove(0);
        let mut titles = vec![Title {
            dvd: None,
            id: 0,
            source_title_id: "1".into(),
            disc_title_number: None,
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
            video_metadata: None,
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
    fn exact_identifier_does_not_accept_an_empty_track_list() {
        let response = serde_json::from_value(serde_json::json!({"releases": [{"id": "release", "title": "Album", "media": [{"position": 1, "discs": [{"id": "disc-id"}], "tracks": []}]}]})).unwrap();
        assert!(
            match_musicbrainz_response(
                response,
                "disc-id",
                None,
                &[TocTrack {
                    number: 1,
                    sectors: 75,
                    start_sector: 0
                }]
            )
            .is_err()
        );
    }
    #[test]
    fn extracts_country_from_common_system_locale_forms() {
        assert_eq!(locale_country("en_AU.UTF-8").as_deref(), Some("AU"));
        assert_eq!(locale_country("pt-BR").as_deref(), Some("BR"));
        assert_eq!(locale_country("C"), None);
    }
}
