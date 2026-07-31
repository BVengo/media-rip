use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use media_rip_core::{Chapter, MediaEngineError};

use super::{
    ProbeCandidate, ProbeInput, locate_child_case_insensitive, normalize_disc_root, source_stem,
};

pub(crate) fn bluray_candidates(path: &Path) -> Result<Vec<ProbeCandidate>, MediaEngineError> {
    let root = normalize_disc_root(path, "BDMV");
    if !root.is_dir() {
        let titles = bluray_disc_titles(&root);
        if !titles.is_empty() {
            let disc_name = source_stem(&root);
            return Ok(titles
                .into_iter()
                .enumerate()
                .map(|(index, metadata)| ProbeCandidate {
                    id: index as u32,
                    source_title_id: format!("{:05}", metadata.playlist),
                    display_name: format!("{disc_name} — Playlist {:05}.mpls", metadata.playlist),
                    input: ProbeInput::BluRay {
                        root: root.clone(),
                        playlist: Some(metadata.playlist),
                    },
                    size_hint: metadata.size,
                    size_hint_is_estimate: false,
                    segment_count: None,
                    segment_map: Some(format!("{:05}.mpls", metadata.playlist)),
                    chapters: metadata.chapters,
                    duplicate_key: metadata.duplicate_key,
                    stream_languages: metadata.stream_languages,
                    authored_stream_pids: metadata.authored_stream_pids,
                    is_main_title: metadata.is_main_title,
                })
                .collect());
        }
        return Ok(vec![ProbeCandidate {
            id: 0,
            source_title_id: "-1".to_owned(),
            display_name: format!("{} — Main playlist", source_stem(&root)),
            input: ProbeInput::BluRay {
                root,
                playlist: None,
            },
            size_hint: None,
            size_hint_is_estimate: true,
            segment_count: None,
            segment_map: None,
            chapters: None,
            duplicate_key: None,
            stream_languages: HashMap::new(),
            authored_stream_pids: None,
            is_main_title: true,
        }]);
    }
    let bdmv = locate_child_case_insensitive(&root, "BDMV").unwrap_or_else(|| root.join("BDMV"));
    let playlists =
        locate_child_case_insensitive(&bdmv, "PLAYLIST").unwrap_or_else(|| bdmv.join("PLAYLIST"));

    let mut playlists = fs::read_dir(&playlists)
        .map_err(|error| {
            MediaEngineError::UnsupportedInput(format!(
                "could not read Blu-ray playlists at {}: {error}",
                playlists.display()
            ))
        })?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_playlist = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("mpls"));
            is_playlist
                .then(|| {
                    path.file_stem()
                        .and_then(|stem| stem.to_str())
                        .and_then(|stem| stem.parse::<u32>().ok())
                        .map(|id| (id, path))
                })
                .flatten()
        })
        .collect::<Vec<_>>();
    playlists.sort_unstable_by_key(|(id, _)| *id);

    if playlists.is_empty() {
        return Err(MediaEngineError::UnsupportedInput(format!(
            "{} does not contain Blu-ray playlists",
            root.display()
        )));
    }

    let playlist_metadata = bluray_playlist_metadata(
        &root,
        &playlists
            .iter()
            .map(|(playlist, _)| *playlist)
            .collect::<Vec<_>>(),
    );
    let disc_name = source_stem(&root);
    Ok(playlists
        .into_iter()
        .enumerate()
        .map(|(index, (playlist, playlist_path))| {
            let items = read_playlist_items(&playlist_path).unwrap_or_default();
            let metadata = playlist_metadata.get(index);
            let exact_size = metadata.and_then(|metadata| metadata.size);
            let segment_count = (!items.is_empty()).then_some(items.len() as u32);
            let segment_map = (!items.is_empty()).then(|| {
                items
                    .iter()
                    .map(|item| format!("{}.m2ts", item.clip_id))
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            ProbeCandidate {
                id: index as u32,
                source_title_id: format!("{playlist:05}"),
                display_name: format!("{disc_name} — Playlist {playlist:05}.mpls"),
                input: ProbeInput::BluRay {
                    root: root.clone(),
                    playlist: Some(playlist),
                },
                size_hint: exact_size,
                size_hint_is_estimate: false,
                segment_count,
                segment_map,
                chapters: metadata.and_then(|metadata| metadata.chapters.clone()),
                duplicate_key: metadata
                    .and_then(|metadata| metadata.duplicate_key.clone())
                    .or_else(|| duplicate_key(&items)),
                stream_languages: metadata
                    .map(|metadata| metadata.stream_languages.clone())
                    .unwrap_or_default(),
                authored_stream_pids: metadata
                    .and_then(|metadata| metadata.authored_stream_pids.clone()),
                is_main_title: metadata.is_some_and(|metadata| metadata.is_main_title),
            }
        })
        .collect())
}

#[derive(Debug, Default)]
struct BlurayPlaylistMetadata {
    playlist: u32,
    size: Option<u64>,
    chapters: Option<Vec<Chapter>>,
    duplicate_key: Option<String>,
    stream_languages: HashMap<u16, String>,
    authored_stream_pids: Option<HashSet<u16>>,
    is_main_title: bool,
}

#[cfg(target_os = "linux")]
fn bluray_disc_titles(root: &Path) -> Vec<BlurayPlaylistMetadata> {
    use std::{ffi::CString, ptr};

    let Ok(root) = CString::new(root.as_os_str().as_encoded_bytes()) else {
        return Vec::new();
    };

    // SAFETY: Each libbluray-owned title-info object is read only while its
    // checked disc handle is open, then freed before that handle is closed.
    unsafe {
        let bluray = bd_open(root.as_ptr(), ptr::null());
        if bluray.is_null() {
            return Vec::new();
        }
        let title_count = bd_get_titles(bluray, 0, 0);
        let main_title = bd_get_main_title(bluray);
        let mut titles = Vec::with_capacity(title_count as usize);
        for index in 0..title_count {
            let info = bd_get_title_info(bluray, index, 0);
            if info.is_null() {
                continue;
            }
            let title_info = &*info.cast::<BlurayTitleInfo>();
            let playlist = title_info.playlist;
            let chapters = chapters_from_title_info(title_info);
            let duplicate_key = duplicate_key_from_title_info(title_info);
            let stream_languages = stream_languages_from_title_info(title_info);
            let authored_stream_pids = authored_stream_pids_from_title_info(title_info);
            let size = (bd_select_title(bluray, index) == 1)
                .then(|| bd_get_title_size(bluray))
                .filter(|size| *size > 0);
            titles.push(BlurayPlaylistMetadata {
                playlist,
                size,
                chapters,
                duplicate_key,
                stream_languages,
                authored_stream_pids,
                is_main_title: main_title >= 0 && index == main_title as u32,
            });
            bd_free_title_info(info);
        }
        bd_close(bluray);
        titles
    }
}

#[cfg(not(target_os = "linux"))]
fn bluray_disc_titles(_root: &Path) -> Vec<BlurayPlaylistMetadata> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn bluray_playlist_metadata(root: &Path, playlists: &[u32]) -> Vec<BlurayPlaylistMetadata> {
    use std::{ffi::CString, ptr};

    let Ok(root) = CString::new(root.as_os_str().as_encoded_bytes()) else {
        return playlists
            .iter()
            .map(|playlist| BlurayPlaylistMetadata {
                playlist: *playlist,
                ..Default::default()
            })
            .collect();
    };

    // SAFETY: libbluray owns the opaque handle. It is checked before use and
    // closed after all playlist queries; no pointers escape this block.
    unsafe {
        let bluray = bd_open(root.as_ptr(), ptr::null());
        if bluray.is_null() {
            return playlists
                .iter()
                .map(|playlist| BlurayPlaylistMetadata {
                    playlist: *playlist,
                    ..Default::default()
                })
                .collect();
        }
        let main_playlist = main_playlist(bluray);
        let metadata = playlists
            .iter()
            .map(|playlist| {
                let size = (bd_select_playlist(bluray, *playlist) == 1)
                    .then(|| bd_get_title_size(bluray))
                    .filter(|size| *size > 0);
                let info = bd_get_playlist_info(bluray, *playlist, 0);
                if info.is_null() {
                    return BlurayPlaylistMetadata {
                        playlist: *playlist,
                        size,
                        ..Default::default()
                    };
                }
                let title_info = &*info.cast::<BlurayTitleInfo>();
                let result = BlurayPlaylistMetadata {
                    playlist: *playlist,
                    size,
                    chapters: chapters_from_title_info(title_info),
                    duplicate_key: duplicate_key_from_title_info(title_info),
                    stream_languages: stream_languages_from_title_info(title_info),
                    authored_stream_pids: authored_stream_pids_from_title_info(title_info),
                    is_main_title: main_playlist == Some(*playlist),
                };
                bd_free_title_info(info);
                result
            })
            .collect();
        bd_close(bluray);
        metadata
    }
}

#[cfg(not(target_os = "linux"))]
fn bluray_playlist_metadata(_root: &Path, playlists: &[u32]) -> Vec<BlurayPlaylistMetadata> {
    playlists
        .iter()
        .map(|playlist| BlurayPlaylistMetadata {
            playlist: *playlist,
            ..Default::default()
        })
        .collect()
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct BlurayTitleInfo {
    _index: u32,
    playlist: u32,
    duration: u64,
    clip_count: u32,
    _angle_count: u8,
    chapter_count: u32,
    _mark_count: u32,
    clips: *const BlurayClipInfo,
    chapters: *const BlurayTitleChapter,
    _marks: *const std::ffi::c_void,
    _mvc_base_view_r_flag: u8,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct BlurayClipInfo {
    _pkt_count: u32,
    _still_mode: u8,
    _still_time: u16,
    video_stream_count: u8,
    audio_stream_count: u8,
    pg_stream_count: u8,
    ig_stream_count: u8,
    sec_audio_stream_count: u8,
    sec_video_stream_count: u8,
    video_streams: *const BlurayStreamInfo,
    audio_streams: *const BlurayStreamInfo,
    pg_streams: *const BlurayStreamInfo,
    ig_streams: *const BlurayStreamInfo,
    sec_audio_streams: *const BlurayStreamInfo,
    sec_video_streams: *const BlurayStreamInfo,
    _start_time: u64,
    in_time: u64,
    out_time: u64,
    clip_id: [std::ffi::c_char; 6],
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct BlurayStreamInfo {
    _coding_type: u8,
    _format: u8,
    _rate: u8,
    _char_code: u8,
    language: [u8; 4],
    pid: u16,
    _aspect: u8,
    _subpath_id: u8,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct BlurayTitleChapter {
    index: u32,
    start: u64,
    duration: u64,
    _offset: u64,
    _clip_ref: std::ffi::c_uint,
}

#[cfg(target_os = "linux")]
#[link(name = "bluray")]
unsafe extern "C" {
    #[link_name = "bd_open"]
    fn bd_open(
        device_path: *const std::ffi::c_char,
        keyfile_path: *const std::ffi::c_char,
    ) -> *mut std::ffi::c_void;
    #[link_name = "bd_close"]
    fn bd_close(bluray: *mut std::ffi::c_void);
    #[link_name = "bd_select_playlist"]
    fn bd_select_playlist(bluray: *mut std::ffi::c_void, playlist: u32) -> i32;
    #[link_name = "bd_get_titles"]
    fn bd_get_titles(bluray: *mut std::ffi::c_void, flags: u8, minimum_seconds: u32) -> u32;
    #[link_name = "bd_get_main_title"]
    fn bd_get_main_title(bluray: *mut std::ffi::c_void) -> i32;
    #[link_name = "bd_get_title_info"]
    fn bd_get_title_info(
        bluray: *mut std::ffi::c_void,
        title_index: u32,
        angle: u32,
    ) -> *mut std::ffi::c_void;
    #[link_name = "bd_get_playlist_info"]
    fn bd_get_playlist_info(
        bluray: *mut std::ffi::c_void,
        playlist: u32,
        angle: u32,
    ) -> *mut std::ffi::c_void;
    #[link_name = "bd_free_title_info"]
    fn bd_free_title_info(title_info: *mut std::ffi::c_void);
    #[link_name = "bd_select_title"]
    fn bd_select_title(bluray: *mut std::ffi::c_void, title_index: u32) -> i32;
    #[link_name = "bd_get_title_size"]
    fn bd_get_title_size(bluray: *mut std::ffi::c_void) -> u64;
}

#[cfg(target_os = "linux")]
unsafe fn main_playlist(bluray: *mut std::ffi::c_void) -> Option<u32> {
    let title_count = unsafe { bd_get_titles(bluray, 0, 0) };
    let main_title = unsafe { bd_get_main_title(bluray) };
    if main_title < 0 || main_title as u32 >= title_count {
        return None;
    }
    let info = unsafe { bd_get_title_info(bluray, main_title as u32, 0) };
    if info.is_null() {
        return None;
    }
    let playlist = unsafe { (*info.cast::<BlurayTitleInfo>()).playlist };
    unsafe { bd_free_title_info(info) };
    Some(playlist)
}

#[cfg(target_os = "linux")]
unsafe fn chapters_from_title_info(info: &BlurayTitleInfo) -> Option<Vec<Chapter>> {
    if info.chapter_count == 0 || info.chapters.is_null() {
        return None;
    }
    // SAFETY: The caller keeps the libbluray title-info allocation alive for
    // this call, and libbluray reports the number of entries in the array.
    let chapters =
        unsafe { std::slice::from_raw_parts(info.chapters, info.chapter_count as usize) };
    Some(
        chapters
            .iter()
            .map(|chapter| Chapter {
                index: chapter.index,
                start_ms: chapter.start / 90,
                end_ms: chapter.start.saturating_add(chapter.duration) / 90,
                name: None,
            })
            .collect(),
    )
}

#[cfg(target_os = "linux")]
unsafe fn duplicate_key_from_title_info(info: &BlurayTitleInfo) -> Option<String> {
    if info.clip_count == 0 || info.clips.is_null() {
        return None;
    }
    // SAFETY: The caller keeps the libbluray title-info allocation alive for
    // this call, and libbluray reports the number of entries in the array.
    let clips = unsafe { std::slice::from_raw_parts(info.clips, info.clip_count as usize) };
    let mut key = String::new();
    for clip in clips {
        let clip_id = clip
            .clip_id
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| *byte as u8 as char)
            .collect::<String>();
        if clip_id.is_empty() {
            return None;
        }
        key.push_str(&format!(
            "{clip_id}:{:016x}:{:016x};",
            clip.in_time, clip.out_time
        ));
    }
    Some(key)
}

#[cfg(target_os = "linux")]
unsafe fn stream_languages_from_title_info(info: &BlurayTitleInfo) -> HashMap<u16, String> {
    if info.clip_count == 0 || info.clips.is_null() {
        return HashMap::new();
    }
    // SAFETY: The title-info allocation owns each clip and stream array and
    // remains alive for this call. Every array length comes from libbluray.
    let clips = unsafe { std::slice::from_raw_parts(info.clips, info.clip_count as usize) };
    let mut languages = HashMap::new();
    for clip in clips {
        for (streams, count) in [
            (clip.video_streams, clip.video_stream_count),
            (clip.audio_streams, clip.audio_stream_count),
            (clip.pg_streams, clip.pg_stream_count),
            (clip.ig_streams, clip.ig_stream_count),
            (clip.sec_audio_streams, clip.sec_audio_stream_count),
            (clip.sec_video_streams, clip.sec_video_stream_count),
        ] {
            if streams.is_null() || count == 0 {
                continue;
            }
            // SAFETY: See the function-level safety comment above.
            for stream in unsafe { std::slice::from_raw_parts(streams, count as usize) } {
                let end = stream
                    .language
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(stream.language.len());
                if let Ok(language) = std::str::from_utf8(&stream.language[..end])
                    && !language.trim().is_empty()
                    && !language.eq_ignore_ascii_case("und")
                {
                    languages
                        .entry(stream.pid)
                        .or_insert_with(|| language.to_owned());
                }
            }
        }
    }
    languages
}

#[cfg(target_os = "linux")]
unsafe fn authored_stream_pids_from_title_info(info: &BlurayTitleInfo) -> Option<HashSet<u16>> {
    if info.clip_count == 0 || info.clips.is_null() {
        return None;
    }
    // SAFETY: The caller keeps the title-info allocation alive, and every
    // stream-array length is supplied by libbluray.
    let clips = unsafe { std::slice::from_raw_parts(info.clips, info.clip_count as usize) };
    let mut pids = HashSet::new();
    for clip in clips {
        for (streams, count) in [
            (clip.video_streams, clip.video_stream_count),
            (clip.audio_streams, clip.audio_stream_count),
            (clip.pg_streams, clip.pg_stream_count),
            (clip.ig_streams, clip.ig_stream_count),
            (clip.sec_audio_streams, clip.sec_audio_stream_count),
            (clip.sec_video_streams, clip.sec_video_stream_count),
        ] {
            if streams.is_null() || count == 0 {
                continue;
            }
            // SAFETY: See the function-level safety comment above.
            for stream in unsafe { std::slice::from_raw_parts(streams, count as usize) } {
                pids.insert(stream.pid);
            }
        }
    }
    (!pids.is_empty()).then_some(pids)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaylistItem {
    clip_id: String,
    in_time: u32,
    out_time: u32,
}

fn read_playlist_items(path: &Path) -> Option<Vec<PlaylistItem>> {
    let bytes = fs::read(path).ok()?;
    playlist_items_from_bytes(&bytes)
}

fn playlist_items_from_bytes(bytes: &[u8]) -> Option<Vec<PlaylistItem>> {
    if bytes.get(..4)? != b"MPLS" {
        return None;
    }
    let playlist_start = read_be_u32(bytes.get(8..12)?)? as usize;
    let play_item_count = read_be_u16(bytes.get(playlist_start + 6..playlist_start + 8)?)? as usize;
    let mut cursor = playlist_start.checked_add(10)?;
    let mut items = Vec::with_capacity(play_item_count);
    for _ in 0..play_item_count {
        let item_length = read_be_u16(bytes.get(cursor..cursor + 2)?)? as usize;
        let item_start = cursor.checked_add(2)?;
        let item_end = item_start.checked_add(item_length)?;
        let clip = std::str::from_utf8(bytes.get(item_start..item_start + 5)?)
            .ok()?
            .trim();
        if clip.len() != 5 || !clip.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let in_time = read_be_u32(bytes.get(item_start + 12..item_start + 16)?)?;
        let out_time = read_be_u32(bytes.get(item_start + 16..item_start + 20)?)?;
        items.push(PlaylistItem {
            clip_id: clip.to_owned(),
            in_time,
            out_time,
        });
        bytes.get(..item_end)?;
        cursor = item_end;
    }
    Some(items)
}

fn duplicate_key(items: &[PlaylistItem]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    Some(
        items
            .iter()
            .map(|item| {
                format!(
                    "{}:{:016x}:{:016x};",
                    item.clip_id, item.in_time, item.out_time
                )
            })
            .collect(),
    )
}

fn read_be_u16(bytes: &[u8]) -> Option<u16> {
    let bytes: [u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn read_be_u32(bytes: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::{PlaylistItem, duplicate_key, playlist_items_from_bytes};

    #[test]
    fn reads_clip_ids_from_mpls_play_items() {
        let mut bytes = vec![0_u8; 96];
        bytes[..8].copy_from_slice(b"MPLS0200");
        bytes[8..12].copy_from_slice(&32_u32.to_be_bytes());
        bytes[38..40].copy_from_slice(&2_u16.to_be_bytes());

        bytes[42..44].copy_from_slice(&20_u16.to_be_bytes());
        bytes[44..53].copy_from_slice(b"00001M2TS");
        bytes[56..60].copy_from_slice(&90_000_u32.to_be_bytes());
        bytes[60..64].copy_from_slice(&180_000_u32.to_be_bytes());
        bytes[64..66].copy_from_slice(&20_u16.to_be_bytes());
        bytes[66..75].copy_from_slice(b"00042M2TS");
        bytes[78..82].copy_from_slice(&270_000_u32.to_be_bytes());
        bytes[82..86].copy_from_slice(&450_000_u32.to_be_bytes());

        assert_eq!(
            playlist_items_from_bytes(&bytes).unwrap(),
            [
                PlaylistItem {
                    clip_id: "00001".into(),
                    in_time: 90_000,
                    out_time: 180_000,
                },
                PlaylistItem {
                    clip_id: "00042".into(),
                    in_time: 270_000,
                    out_time: 450_000,
                }
            ]
        );
    }

    #[test]
    fn duplicate_key_includes_clip_ids_and_exact_ranges() {
        let items = [PlaylistItem {
            clip_id: "00001".into(),
            in_time: 90_000,
            out_time: 180_000,
        }];

        assert_eq!(
            duplicate_key(&items).as_deref(),
            Some("00001:0000000000015f90:000000000002bf20;")
        );
    }
}
