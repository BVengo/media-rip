use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use md5::{Digest, Md5};
use media_rip_core::{DiscKind, MediaEngineError, Title, ports::CancellationToken};
use serde::{Deserialize, Serialize};

use super::cache::MetadataCache;
use crate::{
    diagnostics,
    source::{locate_child_case_insensitive, normalize_disc_root},
};

const DISCDB_GRAPHQL_URL: &str = "https://thediscdb.com/graphql/";
const CACHE_NAMESPACE: &str = "discdb-raw-v1";
const HASH_LOOKUP_QUERY: &str = r#"
query LookupByHash($hash: String!) {
  mediaItems(
    where: {
      releases: { some: { discs: { some: { disc: { contentHash: { eq: $hash } } } } } }
    }
  ) {
    nodes {
      title
      type
      year
      slug
      externalids { tmdb imdb }
      releases {
        slug
        discs {
          contentHash
          slug
          titles {
            index
            duration
            size
            item { title type season episode }
          }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GraphQlEnvelope {
    data: Option<DiscDbData>,
    #[serde(default)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscDbData {
    #[serde(rename = "mediaItems")]
    media_items: DiscDbNodes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscDbNodes {
    #[serde(default)]
    nodes: Vec<DiscDbMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscDbMedia {
    title: String,
    #[serde(rename = "type", default)]
    media_type: String,
    year: Option<u32>,
    slug: Option<String>,
    #[serde(default)]
    externalids: ExternalIds,
    #[serde(default)]
    releases: Vec<DiscDbRelease>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ExternalIds {
    tmdb: Option<String>,
    imdb: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscDbRelease {
    #[allow(dead_code)]
    slug: Option<String>,
    #[serde(default)]
    discs: Vec<DiscDbDisc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscDbDisc {
    #[serde(rename = "contentHash")]
    content_hash: String,
    slug: Option<String>,
    #[serde(default)]
    titles: Vec<DiscDbTitle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DiscDbTitle {
    pub index: u32,
    #[serde(default)]
    pub duration: String,
    #[serde(default)]
    pub size: u64,
    pub item: Option<DiscDbItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DiscDbItem {
    #[serde(default)]
    pub title: String,
    #[serde(rename = "type", default)]
    pub item_type: String,
    pub season: Option<String>,
    pub episode: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct DiscDbMatch {
    pub title: String,
    pub media_type: String,
    pub year: Option<u32>,
    pub tmdb_id: Option<u64>,
    pub imdb_id: Option<String>,
    pub disc_slug: Option<String>,
    pub titles: Vec<DiscDbTitle>,
}

pub(super) fn content_hash(path: &Path, kind: DiscKind) -> Option<String> {
    let root = match kind {
        DiscKind::BluRay => normalize_disc_root(path, "BDMV"),
        DiscKind::Dvd => normalize_disc_root(path, "VIDEO_TS"),
        _ => return None,
    };
    if !root.is_dir() {
        if kind != DiscKind::Dvd {
            return None;
        }
        let files = iso9660_video_ts_files(path)?;
        log_hash_inputs(path, kind, &files);
        return hash_sizes(files.into_iter().map(|(_, size)| size).collect());
    }
    let (directory, extension) = match kind {
        DiscKind::BluRay => {
            let bdmv =
                locate_child_case_insensitive(&root, "BDMV").unwrap_or_else(|| root.join("BDMV"));
            let stream = locate_child_case_insensitive(&bdmv, "STREAM")?;
            (stream, Some("m2ts"))
        }
        DiscKind::Dvd => {
            let video_ts = locate_child_case_insensitive(&root, "VIDEO_TS")
                .unwrap_or_else(|| root.join("VIDEO_TS"));
            (video_ts, None)
        }
        _ => return None,
    };
    let mut files = fs::read_dir(directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            extension.is_none_or(|extension| {
                path.extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(extension))
            })
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    if files.is_empty() {
        return None;
    }
    let files = files
        .into_iter()
        .map(|file| {
            let size = fs::metadata(&file).ok()?.len();
            Some((file.to_string_lossy().into_owned(), size))
        })
        .collect::<Option<Vec<_>>>()?;
    log_hash_inputs(path, kind, &files);
    hash_sizes(files.into_iter().map(|(_, size)| size).collect())
}

fn log_hash_inputs(path: &Path, kind: DiscKind, files: &[(String, u64)]) {
    diagnostics::disc_json(
        "MediaRip",
        "ordered TheDiscDB content-hash inputs",
        &serde_json::json!({
            "source_path": path,
            "disc_kind": kind,
            "files": files.iter().enumerate().map(|(order, (name, size))| serde_json::json!({
                "order": order,
                "name": name,
                "size_bytes": size,
            })).collect::<Vec<_>>(),
        }),
    );
}

fn hash_sizes(sizes: Vec<u64>) -> Option<String> {
    if sizes.is_empty() {
        return None;
    }
    let mut digest = Md5::new();
    for size in sizes {
        digest.update(i64::try_from(size).ok()?.to_le_bytes());
    }
    Some(format!("{:X}", digest.finalize()))
}

const ISO_SECTOR_SIZE: u64 = 2_048;

#[derive(Debug, PartialEq, Eq)]
struct IsoEntry {
    name: String,
    extent: u32,
    size: u32,
    directory: bool,
}

/// Reads the ISO9660 bridge directory present on ordinary video DVDs. This is
/// deliberately directory-table-only: no movie content is read or uploaded.
fn iso9660_video_ts_files(path: &Path) -> Option<Vec<(String, u64)>> {
    let mut image = fs::File::open(path).ok()?;
    let mut descriptor = [0_u8; ISO_SECTOR_SIZE as usize];
    let root = (16_u64..=255).find_map(|sector| {
        image.seek(SeekFrom::Start(sector * ISO_SECTOR_SIZE)).ok()?;
        image.read_exact(&mut descriptor).ok()?;
        if &descriptor[1..6] != b"CD001" {
            return None;
        }
        match descriptor[0] {
            1 => parse_iso_entry(&descriptor[156..]),
            255 => None,
            _ => None,
        }
    })?;
    let root_entries = read_iso_directory(&mut image, &root)?;
    let video_ts = root_entries
        .iter()
        .find(|entry| entry.directory && entry.name.eq_ignore_ascii_case("VIDEO_TS"))?;
    let mut files = read_iso_directory(&mut image, video_ts)?
        .into_iter()
        .filter(|entry| !entry.directory)
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| entry.name.to_ascii_uppercase());
    Some(
        files
            .into_iter()
            .map(|entry| (entry.name, u64::from(entry.size)))
            .collect(),
    )
}

fn read_iso_directory(image: &mut fs::File, directory: &IsoEntry) -> Option<Vec<IsoEntry>> {
    let mut data = vec![0_u8; usize::try_from(directory.size).ok()?];
    image
        .seek(SeekFrom::Start(
            u64::from(directory.extent) * ISO_SECTOR_SIZE,
        ))
        .ok()?;
    image.read_exact(&mut data).ok()?;
    Some(parse_iso_directory(&data))
}

fn parse_iso_directory(data: &[u8]) -> Vec<IsoEntry> {
    let mut entries = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let length = usize::from(data[offset]);
        if length == 0 {
            offset = ((offset / ISO_SECTOR_SIZE as usize) + 1) * ISO_SECTOR_SIZE as usize;
            continue;
        }
        if offset + length > data.len() {
            break;
        }
        if let Some(entry) = parse_iso_entry(&data[offset..offset + length])
            && entry.name != "\0"
            && entry.name != "\u{1}"
        {
            entries.push(entry);
        }
        offset += length;
    }
    entries
}

fn parse_iso_entry(record: &[u8]) -> Option<IsoEntry> {
    let length = usize::from(*record.first()?);
    if length < 34 || record.len() < length {
        return None;
    }
    let name_length = usize::from(record[32]);
    let name = std::str::from_utf8(record.get(33..33 + name_length)?)
        .ok()?
        .split(';')
        .next()?
        .trim_end_matches('.')
        .to_owned();
    Some(IsoEntry {
        name,
        extent: u32::from_le_bytes(record[2..6].try_into().ok()?),
        size: u32::from_le_bytes(record[10..14].try_into().ok()?),
        directory: record[25] & 0x02 != 0,
    })
}

pub(super) fn lookup(
    content_hash: &str,
    cache: &MetadataCache,
    cancellation: &CancellationToken,
) -> Result<Option<DiscDbMatch>, String> {
    if cancellation.is_cancelled() {
        return Err(MediaEngineError::Cancelled.to_string());
    }
    log::debug!(
        target: "media_rip_engine::metadata_decision",
        "TheDiscDB lookup planned: method=exact content hash; hash={content_hash}; hash_origin=ordered authored disc file sizes; title_query=none"
    );
    let cache_key = format!("hash:{content_hash}");
    let (response_value, response_source) =
        if let Some(cached) = cache.read::<serde_json::Value>(CACHE_NAMESPACE, &cache_key) {
            (cached, "cache")
        } else {
            let request = serde_json::json!({
                "query": HASH_LOOKUP_QUERY,
                "variables": { "hash": content_hash }
            });
            let network_response = ureq::post(DISCDB_GRAPHQL_URL)
                .set(
                    "User-Agent",
                    concat!("MediaRip/", env!("CARGO_PKG_VERSION")),
                )
                .set("Accept", "application/json")
                .timeout(Duration::from_secs(10))
                .send_json(request)
                .map_err(|error| error.to_string())
                .and_then(|response| {
                    response
                        .into_json::<serde_json::Value>()
                        .map_err(|error| error.to_string())
                });
            match network_response {
                Ok(response) => (response, "network"),
                Err(error) => {
                    if let Some(stale) =
                        cache.read_stale::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
                    {
                        (stale, "stale-cache")
                    } else {
                        return Err(error.to_string());
                    }
                }
            }
        };
    diagnostics::json(
        "TheDiscDB",
        "lookup by content hash",
        response_source,
        &response_value,
    );
    let response: GraphQlEnvelope = serde_json::from_value(response_value.clone())
        .map_err(|error| format!("invalid TheDiscDB response: {error}"))?;
    if !response.errors.is_empty() {
        return Err(format!(
            "TheDiscDB returned {} GraphQL error(s)",
            response.errors.len()
        ));
    }
    if response_source == "network"
        && find_exact_match(response.data.clone(), content_hash).is_some()
    {
        cache.write(CACHE_NAMESPACE, &cache_key, &response_value);
    }
    Ok(find_exact_match(response.data, content_hash))
}

fn find_exact_match(data: Option<DiscDbData>, hash: &str) -> Option<DiscDbMatch> {
    for media in data?.media_items.nodes {
        for release in media.releases {
            for disc in release.discs {
                if disc.content_hash.eq_ignore_ascii_case(hash) {
                    return Some(DiscDbMatch {
                        title: media.title,
                        media_type: media.media_type,
                        year: media.year,
                        tmdb_id: media.externalids.tmdb.and_then(|value| value.parse().ok()),
                        imdb_id: media.externalids.imdb,
                        disc_slug: disc.slug,
                        titles: disc.titles,
                    });
                }
            }
        }
    }
    None
}

pub(super) fn matched_title_indices(
    mappings: &[DiscDbTitle],
    titles: &[Title],
) -> Vec<(usize, usize)> {
    let mut candidates = Vec::<(u64, usize, usize)>::new();
    for (mapping_index, mapping) in mappings.iter().enumerate() {
        if mapping.item.is_none() {
            continue;
        }
        let expected_ms = parse_duration(&mapping.duration).saturating_mul(1_000);
        for (title_index, title) in titles.iter().enumerate() {
            let duration_error = title.duration_ms.abs_diff(expected_ms);
            if expected_ms > 0 && duration_error > 3_000 {
                continue;
            }
            let size_error = match (mapping.size, title.size_bytes) {
                (expected, Some(actual)) if expected > 0 => {
                    let difference = expected.abs_diff(actual);
                    if difference.saturating_mul(100) > expected.saturating_mul(5) {
                        continue;
                    }
                    difference.saturating_mul(1_000) / expected.max(1)
                }
                _ if expected_ms == 0 => continue,
                _ => 100,
            };
            let index_hint = if title.disc_title_number == Some(mapping.index.saturating_add(1)) {
                0
            } else if title.id == mapping.index {
                5
            } else if title.source_title_id.parse::<u32>().ok() == Some(mapping.index) {
                10
            } else {
                20
            };
            let score = duration_error
                .saturating_add(size_error.saturating_mul(10))
                .saturating_add(index_hint);
            candidates.push((score, mapping_index, title_index));
        }
    }
    candidates.sort_unstable();
    let mut used_mappings = vec![false; mappings.len()];
    let mut used_titles = vec![false; titles.len()];
    let mut matches = Vec::new();
    for (score, mapping, title) in candidates.iter().copied() {
        if used_mappings[mapping] || used_titles[title] {
            continue;
        }
        let authoritative_title_number =
            titles[title].disc_title_number == Some(mappings[mapping].index.saturating_add(1));
        let ambiguous = !authoritative_title_number
            && candidates
                .iter()
                .any(|(other_score, other_mapping, other_title)| {
                    *other_mapping == mapping
                        && *other_title != title
                        && *other_score <= score.saturating_add(250)
                });
        if ambiguous {
            continue;
        }
        used_mappings[mapping] = true;
        used_titles[title] = true;
        matches.push((mapping, title));
    }
    matches
}

pub(super) fn parse_duration(value: &str) -> u64 {
    let parts = value
        .split(':')
        .filter_map(|part| part.parse::<u64>().ok())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [hours, minutes, seconds] => hours * 3_600 + minutes * 60 + seconds,
        [minutes, seconds] => minutes * 60 + seconds,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use media_rip_core::Title;

    use super::{
        DiscDbItem, DiscDbTitle, GraphQlEnvelope, content_hash, find_exact_match,
        matched_title_indices, parse_duration, parse_iso_directory,
    };

    fn iso_record(name: &str, extent: u32, size: u32, directory: bool) -> Vec<u8> {
        let name = name.as_bytes();
        let length = 33 + name.len() + usize::from(name.len().is_multiple_of(2));
        let mut record = vec![0_u8; length];
        record[0] = u8::try_from(length).unwrap();
        record[2..6].copy_from_slice(&extent.to_le_bytes());
        record[10..14].copy_from_slice(&size.to_le_bytes());
        record[25] = if directory { 0x02 } else { 0 };
        record[32] = u8::try_from(name.len()).unwrap();
        record[33..33 + name.len()].copy_from_slice(name);
        record
    }

    fn item() -> Option<DiscDbItem> {
        Some(DiscDbItem {
            title: "Episode".into(),
            item_type: "Episode".into(),
            season: Some("1".into()),
            episode: Some("1".into()),
        })
    }

    fn title(id: u32, source_id: &str, duration_ms: u64, size: u64) -> Title {
        Title {
            id,
            source_title_id: source_id.into(),
            disc_title_number: None,
            name: source_id.into(),
            is_main_title: false,
            duration_ms,
            size_bytes: Some(size),
            size_is_estimate: false,
            chapters: Vec::new(),
            tracks: Vec::new(),
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            video_metadata: None,
            output_file: String::new(),
            selected: false,
        }
    }

    #[test]
    fn computes_thediscdb_hash_from_sorted_little_endian_sizes() {
        let root = std::env::temp_dir().join(format!("mediarip-disc-hash-{}", std::process::id()));
        let stream = root.join("BDMV/STREAM");
        fs::create_dir_all(&stream).unwrap();
        fs::write(stream.join("00002.m2ts"), vec![0; 2]).unwrap();
        fs::write(stream.join("00001.m2ts"), vec![0; 1]).unwrap();

        assert_eq!(
            content_hash(&root, media_rip_core::DiscKind::BluRay).as_deref(),
            Some("CA57733A518406F3197F92EC197B0A0A")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_iso9660_dvd_directory_records() {
        let mut directory = iso_record("VIDEO_TS.IFO;1", 42, 24_576, false);
        directory.extend(iso_record("VTS_01_1.VOB;1", 54, 1_073_725_440, false));
        directory.extend(iso_record("EXTRAS", 60, 2_048, true));

        let entries = parse_iso_directory(&directory);

        assert_eq!(entries[0].name, "VIDEO_TS.IFO");
        assert_eq!(entries[0].size, 24_576);
        assert_eq!(entries[1].name, "VTS_01_1.VOB");
        assert!(entries[2].directory);
    }

    #[test]
    fn matches_only_unambiguous_duration_and_size_pairs() {
        let mappings = vec![DiscDbTitle {
            index: 7,
            duration: "0:42:00".into(),
            size: 1_000,
            item: item(),
        }];
        let titles = vec![
            title(0, "00800", 2_520_000, 1_000),
            title(1, "00801", 1_200_000, 500),
        ];

        assert_eq!(matched_title_indices(&mappings, &titles), [(0, 0)]);
    }

    #[test]
    fn rejects_ambiguous_same_length_titles() {
        let mappings = vec![DiscDbTitle {
            index: 9,
            duration: "42:00".into(),
            size: 0,
            item: item(),
        }];
        let titles = vec![
            title(0, "00800", 2_520_000, 1_000),
            title(1, "00801", 2_520_000, 1_000),
        ];

        assert!(matched_title_indices(&mappings, &titles).is_empty());
        assert_eq!(parse_duration("1:13:14"), 4_394);
    }

    #[test]
    fn authored_title_number_disambiguates_duplicate_playlists() {
        let mappings = vec![DiscDbTitle {
            index: 0,
            duration: "42:00".into(),
            size: 1_000,
            item: item(),
        }];
        let mut titles = vec![
            title(4, "00804", 2_520_000, 1_000),
            title(0, "00800", 2_520_000, 1_000),
        ];
        titles[1].disc_title_number = Some(1);

        assert_eq!(matched_title_indices(&mappings, &titles), [(0, 1)]);
    }

    #[test]
    fn parses_the_production_graphql_shape_and_large_sizes() {
        let response: GraphQlEnvelope = serde_json::from_str(
            r#"{"data":{"mediaItems":{"nodes":[{"title":"Band of Brothers","type":"Series","year":2001,"slug":"band-of-brothers","externalids":{"tmdb":"4613","imdb":"tt0185906"},"releases":[{"slug":"release","discs":[{"contentHash":"ABC","slug":"S01D01","titles":[{"index":0,"duration":"1:13:14","size":18405949440,"item":{"title":"Currahee","type":"Episode","season":"1","episode":"1"}}]}]}]}]}}}"#,
        )
        .unwrap();

        let matched = find_exact_match(response.data, "abc").unwrap();
        assert_eq!(matched.title, "Band of Brothers");
        assert_eq!(matched.tmdb_id, Some(4613));
        assert_eq!(matched.imdb_id.as_deref(), Some("tt0185906"));
        assert_eq!(matched.titles[0].size, 18_405_949_440);
    }
}
