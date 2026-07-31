use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The physical or logical form of an input source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscKind {
    Dvd,
    BluRay,
    Directory,
    File,
    Unknown,
}

impl DiscKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Dvd => "DVD",
            Self::BluRay => "Blu-ray",
            Self::Directory => "Video folder",
            Self::File => "Media file",
            Self::Unknown => "Unknown media",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Dvd => "media-optical-dvd-symbolic",
            Self::BluRay => "media-optical-bd-symbolic",
            Self::Directory => "folder-videos-symbolic",
            Self::File => "video-x-generic-symbolic",
            Self::Unknown => "media-optical-symbolic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drive {
    pub device_path: PathBuf,
    pub mount_path: Option<PathBuf>,
    pub display_name: String,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub revision: Option<String>,
    pub serial: Option<String>,
    pub bus: Option<String>,
    pub media_kind: DiscKind,
    pub media_label: Option<String>,
    pub capacity_bytes: Option<u64>,
}

impl Drive {
    pub fn empty(device_path: impl Into<PathBuf>, display_name: impl Into<String>) -> Self {
        Self {
            device_path: device_path.into(),
            mount_path: None,
            display_name: display_name.into(),
            vendor: None,
            model: None,
            revision: None,
            serial: None,
            bus: None,
            media_kind: DiscKind::Unknown,
            media_label: None,
            capacity_bytes: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaSource {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub kind: DiscKind,
    pub drive: Option<Drive>,
    pub capacity_bytes: Option<u64>,
    pub encrypted: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Attachment,
    Data,
}

impl StreamKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Subtitle => "Subtitles",
            Self::Attachment => "Attachment",
            Self::Data => "Data",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Video => "video-x-generic-symbolic",
            Self::Audio => "audio-x-generic-symbolic",
            Self::Subtitle => "insert-text-symbolic",
            Self::Attachment => "mail-attachment-symbolic",
            Self::Data => "application-x-generic-symbolic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub index: u32,
    pub kind: StreamKind,
    pub codec: String,
    pub codec_long_name: Option<String>,
    pub language: Option<String>,
    pub name: Option<String>,
    pub properties: Vec<String>,
    pub selected: bool,
}

impl Track {
    pub fn summary(&self) -> String {
        let mut parts = vec![self.codec.clone()];
        if let Some(language) = self.language.as_deref().filter(|value| !value.is_empty()) {
            parts.push(language.to_owned());
        }
        parts.extend(
            self.properties
                .iter()
                .filter(|item| !item.is_empty())
                .cloned(),
        );
        parts.join("  ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Title {
    pub id: u32,
    pub source_title_id: String,
    pub name: String,
    #[serde(default)]
    pub is_main_title: bool,
    pub duration_ms: u64,
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub size_is_estimate: bool,
    pub chapters: Vec<Chapter>,
    pub tracks: Vec<Track>,
    pub segment_count: Option<u32>,
    pub segment_map: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_key: Option<String>,
    pub output_file: String,
    pub selected: bool,
}

impl Title {
    pub fn selected_track_count(&self) -> usize {
        self.tracks.iter().filter(|track| track.selected).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfoField {
    pub label: String,
    pub value: String,
}

impl InfoField {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaCatalog {
    pub source: MediaSource,
    pub titles: Vec<Title>,
    pub source_info: Vec<InfoField>,
}
