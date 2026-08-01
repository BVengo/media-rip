use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::MediaCatalog;

/// A complete, user-selectable output strategy.
///
/// Profiles deliberately describe the result rather than FFmpeg flags so other
/// engine backends and frontends can share the same request model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OutputProfile {
    #[default]
    PreserveMkv,
    H264Mp4,
    H265Mkv,
    Av1Webm,
    AudioFlac,
    AudioWav,
    AudioOpus,
    AudioAac,
}

impl OutputProfile {
    pub const ALL: [Self; 8] = [
        Self::PreserveMkv,
        Self::H264Mp4,
        Self::H265Mkv,
        Self::Av1Webm,
        Self::AudioFlac,
        Self::AudioWav,
        Self::AudioOpus,
        Self::AudioAac,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::PreserveMkv => "copy-mkv",
            Self::H264Mp4 => "h264-mp4",
            Self::H265Mkv => "h265-mkv",
            Self::Av1Webm => "av1-webm",
            Self::AudioFlac => "flac-audio",
            Self::AudioWav => "wav-audio",
            Self::AudioOpus => "opus-audio",
            Self::AudioAac => "aac-audio",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|profile| profile.id() == value)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::PreserveMkv => "MKV — preserve original tracks",
            Self::H264Mp4 => "MP4 — H.264 video + AAC audio",
            Self::H265Mkv => "MKV — H.265 video",
            Self::Av1Webm => "WebM — AV1 video + Opus audio",
            Self::AudioFlac => "FLAC — lossless audio",
            Self::AudioWav => "WAV — uncompressed audio",
            Self::AudioOpus => "Opus — compact audio",
            Self::AudioAac => "M4A — AAC audio",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::PreserveMkv => "Fast, lossless stream copy. Best for archiving video discs.",
            Self::H264Mp4 => "Widely compatible video; re-encodes video, audio and text subtitles.",
            Self::H265Mkv => "Smaller video files; re-encodes video and preserves other tracks.",
            Self::Av1Webm => {
                "Efficient open video format; re-encodes video, audio and text subtitles."
            }
            Self::AudioFlac => "Lossless audio for music, audiobooks and Audio CD tracks.",
            Self::AudioWav => "Uncompressed PCM audio for editing and maximum compatibility.",
            Self::AudioOpus => "Efficient lossy audio for spoken word and portable listening.",
            Self::AudioAac => "Broadly compatible lossy audio in an M4A container.",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::PreserveMkv | Self::H265Mkv => "mkv",
            Self::H264Mp4 => "mp4",
            Self::Av1Webm => "webm",
            Self::AudioFlac => "flac",
            Self::AudioWav => "wav",
            Self::AudioOpus => "opus",
            Self::AudioAac => "m4a",
        }
    }

    pub fn is_audio_only(self) -> bool {
        matches!(
            self,
            Self::AudioFlac | Self::AudioWav | Self::AudioOpus | Self::AudioAac
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RipSelection {
    pub title_id: u32,
    pub stream_indices: Vec<u32>,
    pub output_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RipRequest {
    pub catalog: MediaCatalog,
    pub selections: Vec<RipSelection>,
    pub output_directory: PathBuf,
    pub overwrite: bool,
    #[serde(default)]
    pub output_profile: OutputProfile,
}

impl RipRequest {
    pub fn from_catalog(catalog: MediaCatalog, output_directory: PathBuf, overwrite: bool) -> Self {
        Self::from_catalog_with_profile(
            catalog,
            output_directory,
            overwrite,
            OutputProfile::default(),
        )
    }

    pub fn from_catalog_with_profile(
        catalog: MediaCatalog,
        output_directory: PathBuf,
        overwrite: bool,
        output_profile: OutputProfile,
    ) -> Self {
        let selections = catalog
            .titles
            .iter()
            .filter(|title| title.selected)
            .map(|title| RipSelection {
                title_id: title.id,
                stream_indices: title
                    .tracks
                    .iter()
                    .filter(|track| track.selected)
                    .map(|track| track.index)
                    .collect(),
                output_file: title.output_file.clone(),
            })
            .collect();

        Self {
            catalog,
            selections,
            output_directory,
            overwrite,
            output_profile,
        }
    }
}
