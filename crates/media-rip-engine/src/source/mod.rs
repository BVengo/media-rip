mod audio_cd;
mod bluray;
mod dvd;

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use media_rip_core::{Chapter, DiscKind, MediaEngineError};

pub(crate) use audio_cd::audio_cd_catalog;
pub(crate) use bluray::bluray_candidates;
pub(crate) use dvd::{dvd_candidates, dvd_text_name};

#[derive(Debug, Clone)]
pub(crate) struct ProbeCandidate {
    pub id: u32,
    pub source_title_id: String,
    pub disc_title_number: Option<u32>,
    pub display_name: String,
    pub input: ProbeInput,
    pub size_hint: Option<u64>,
    pub size_hint_is_estimate: bool,
    pub segment_count: Option<u32>,
    pub segment_map: Option<String>,
    pub chapters: Option<Vec<Chapter>>,
    pub duplicate_key: Option<String>,
    pub stream_languages: HashMap<u16, String>,
    pub authored_stream_pids: Option<HashSet<u16>>,
    pub is_main_title: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ProbeInput {
    File(PathBuf),
    Dvd {
        root: PathBuf,
        title: u32,
        info: Option<media_rip_core::model::dvd::DvdTitleInfo>,
    },
    BluRay {
        root: PathBuf,
        playlist: Option<u32>,
    },
}

impl ProbeInput {
    pub fn ffmpeg_input_args(&self) -> Vec<String> {
        match self {
            Self::File(path) => vec!["-i".to_owned(), path.to_string_lossy().into_owned()],
            Self::Dvd { root, title, info } => {
                dvd_input_args(root, &title.to_string(), info.as_ref())
            }
            Self::BluRay { root, playlist } => {
                let mut args = Vec::new();
                if let Some(playlist) = playlist {
                    args.extend(["-playlist".to_owned(), playlist.to_string()]);
                }
                args.extend([
                    "-i".to_owned(),
                    format!("bluray:{}", root.to_string_lossy()),
                ]);
                args
            }
        }
    }
}

/// Scanning and ripping must use identical navigation and chapter boundaries.
pub(crate) fn dvd_input_args(
    root: &Path,
    title: &str,
    info: Option<&media_rip_core::model::dvd::DvdTitleInfo>,
) -> Vec<String> {
    let mut args = vec![
        "-f".to_owned(),
        "dvdvideo".to_owned(),
        "-title".to_owned(),
        title.to_owned(),
        "-trim".to_owned(),
        "1".to_owned(),
    ];
    if let Some(range) = info.and_then(|info| info.chapter_range) {
        args.extend([
            "-chapter_start".to_owned(),
            range.first.to_string(),
            "-chapter_end".to_owned(),
            range.last.to_string(),
        ]);
    }
    args.extend(["-i".to_owned(), root.to_string_lossy().into_owned()]);
    args
}

pub fn detect_kind(path: &Path) -> DiscKind {
    if path.is_dir() {
        if locate_child_case_insensitive(path, "VIDEO_TS").is_some()
            || file_name_eq(path, "VIDEO_TS")
        {
            return DiscKind::Dvd;
        }
        if locate_child_case_insensitive(path, "BDMV").is_some() || file_name_eq(path, "BDMV") {
            return DiscKind::BluRay;
        }
        return DiscKind::Directory;
    }

    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("iso") => DiscKind::Unknown,
        Some(_) => DiscKind::File,
        None if path.starts_with("/dev") => DiscKind::Unknown,
        None => DiscKind::File,
    }
}

pub(crate) fn candidates_for(
    path: &Path,
    kind: DiscKind,
) -> Result<Vec<ProbeCandidate>, MediaEngineError> {
    match kind {
        DiscKind::Dvd => dvd_candidates(path),
        DiscKind::BluRay => bluray_candidates(path),
        DiscKind::AudioCd => unreachable!("Audio CDs are catalogued directly from their TOC"),
        DiscKind::File | DiscKind::Directory | DiscKind::Unknown => {
            let size_hint = fs::metadata(path).ok().map(|metadata| metadata.len());
            Ok(vec![ProbeCandidate {
                id: 0,
                source_title_id: "0".to_owned(),
                disc_title_number: None,
                display_name: source_stem(path),
                input: ProbeInput::File(path.to_owned()),
                size_hint,
                size_hint_is_estimate: false,
                segment_count: None,
                segment_map: None,
                chapters: None,
                duplicate_key: None,
                stream_languages: HashMap::new(),
                authored_stream_pids: None,
                is_main_title: false,
            }])
        }
    }
}

pub(crate) fn normalize_disc_root(path: &Path, marker: &str) -> PathBuf {
    if file_name_eq(path, marker) {
        path.parent().unwrap_or(path).to_owned()
    } else {
        path.to_owned()
    }
}

pub(crate) fn locate_child_case_insensitive(path: &Path, wanted: &str) -> Option<PathBuf> {
    fs::read_dir(path).ok()?.flatten().find_map(|entry| {
        entry
            .file_name()
            .to_str()
            .filter(|name| name.eq_ignore_ascii_case(wanted))
            .map(|_| entry.path())
    })
}

pub(crate) fn source_stem(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Disc")
        .replace('_', " ")
}

fn file_name_eq(path: &Path, wanted: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
}
