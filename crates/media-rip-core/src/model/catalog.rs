use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaTag {
    pub key: String,
    pub value: String,
}

/// Identifies where an automatically applied name or description came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataAttribution {
    pub provider: String,
    pub confidence_percent: u8,
    pub match_method: String,
}

impl MetadataAttribution {
    pub fn new(
        provider: impl Into<String>,
        confidence_percent: u8,
        match_method: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            confidence_percent: confidence_percent.min(100),
            match_method: match_method.into(),
        }
    }
}

/// Structured video metadata associated with one DVD title or Blu-ray playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub attribution: MetadataAttribution,
    pub content_type: Option<String>,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub tmdb_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvdb_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tvmaze_id: Option<u64>,
}

/// A lower-confidence video title proposal that is never applied automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoMetadataSuggestion {
    pub title_id: u32,
    pub name: String,
    pub tags: Vec<MediaTag>,
    pub metadata: VideoMetadata,
    /// Observations which led to this proposal. These are displayed before the
    /// user chooses whether to apply the lower-confidence metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// Heuristic choices which cannot be proven from the disc itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
    /// Other episodes from the resolved season. Frontends use these choices to
    /// shift an otherwise ambiguous consecutive proposal without another
    /// network request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episode_choices: Vec<VideoEpisodeChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoEpisodeChoice {
    pub episode: u32,
    pub name: String,
    pub tags: Vec<MediaTag>,
}

/// A resolved series and season retained even when disc-title matching could
/// not propose an episode automatically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoEpisodeContext {
    pub metadata: VideoMetadata,
    pub choices: Vec<VideoEpisodeChoice>,
}

impl MediaTag {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Source artwork: album art for Audio CDs or disc/poster art for video media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverArt {
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// A per-track choice that differs from the Audio CD's shared album artwork.
/// Missing entries inherit the album artwork; `Removed` explicitly suppresses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackCoverArt {
    Removed,
    Custom(CoverArt),
}

impl CoverArt {
    pub fn new(mime_type: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            mime_type: mime_type.into(),
            data,
        }
    }
}

/// The physical or logical form of an input source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscKind {
    Dvd,
    BluRay,
    AudioCd,
    Directory,
    File,
    Unknown,
}

impl DiscKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Dvd => "DVD",
            Self::BluRay => "Blu-ray",
            Self::AudioCd => "Audio CD",
            Self::Directory => "Video folder",
            Self::File => "Media file",
            Self::Unknown => "Unknown media",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Dvd => "media-optical-dvd-symbolic",
            Self::BluRay => "media-optical-bd-symbolic",
            Self::AudioCd => "media-optical-cd-audio-symbolic",
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
    #[serde(default)]
    pub media_present: bool,
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
            media_present: false,
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
        if let Some(name) = self.name.as_deref().filter(|value| !value.is_empty()) {
            parts.push(name.to_owned());
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_title_number: Option<u32>,
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
    /// Output-container metadata. Audio CD scans populate these values and the
    /// frontend may edit them before a rip starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<MediaTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_metadata: Option<VideoMetadata>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_art: Option<CoverArt>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub track_cover_art: BTreeMap<u32, TrackCoverArt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metadata_attributions: Vec<MetadataAttribution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_metadata_suggestions: Vec<VideoMetadataSuggestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_episode_context: Option<VideoEpisodeContext>,
}

impl MediaCatalog {
    pub fn cover_art_for_title(&self, title_id: u32) -> Option<&CoverArt> {
        match self.track_cover_art.get(&title_id) {
            None => self.cover_art.as_ref(),
            Some(TrackCoverArt::Removed) => None,
            Some(TrackCoverArt::Custom(cover_art)) => Some(cover_art),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StreamKind, Track};

    #[test]
    fn track_summary_includes_an_edited_name() {
        let track = Track {
            index: 1,
            kind: StreamKind::Audio,
            codec: "aac".to_owned(),
            codec_long_name: None,
            language: Some("eng".to_owned()),
            name: Some("Commentary".to_owned()),
            properties: vec!["stereo".to_owned()],
            selected: true,
        };

        assert_eq!(track.summary(), "aac  eng  Commentary  stereo");
    }
}
