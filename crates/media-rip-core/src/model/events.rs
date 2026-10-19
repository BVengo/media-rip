use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{MediaCatalog, VideoEpisodeChoice};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Success,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RipProgress {
    pub current_title: usize,
    pub title_count: usize,
    pub title_name: String,
    pub fraction: f64,
    pub bytes_written: Option<u64>,
    pub read_rate_bytes: Option<u64>,
    pub elapsed_seconds: u64,
    pub remaining_seconds: Option<u64>,
    pub destination: PathBuf,
}

/// The durable result of saving one title in a potentially multi-title job.
///
/// Frontends can use the stable title ID to remove completed work from the
/// active selection, making a failed or cancelled job safe to resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RipTitleResult {
    pub title_id: u32,
    pub title_name: String,
    pub destination: PathBuf,
    pub bytes_written: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RipPhase {
    Reading,
    Encoding,
    Finalising,
}

impl RipPhase {
    pub fn description(self) -> &'static str {
        match self {
            Self::Reading => "Reading disc audio",
            Self::Encoding => "Writing media and metadata",
            Self::Finalising => "Finalising output",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineEvent {
    Log {
        level: LogLevel,
        message: String,
    },
    ScanStarted {
        source: PathBuf,
    },
    ScanProgress {
        current_title: usize,
        title_count: usize,
        source_title_id: String,
    },
    ScanFinished(Box<MediaCatalog>),
    MetadataIdentified(Box<MediaCatalog>),
    MetadataIdentificationFailed(String),
    ScanFailed(String),
    ScanCancelled,
    EpisodeSeasonRefreshed {
        request_id: u64,
        season: u32,
        choices: Vec<VideoEpisodeChoice>,
    },
    EpisodeSeasonRefreshFailed {
        request_id: u64,
        season: u32,
        error: String,
    },
    RipStarted {
        title_count: usize,
    },
    RipProgress(RipProgress),
    RipPhaseChanged {
        phase: RipPhase,
        title_name: String,
    },
    TitleFinished(RipTitleResult),
    RipFinished {
        saved_titles: usize,
        destination: PathBuf,
    },
    RipFailed(String),
    RipCancelled,
}
