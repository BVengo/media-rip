use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
};

use thiserror::Error;

use crate::{Drive, EngineEvent, RipRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviousDvdMatch {
    pub title: String,
    pub metadata_url: Option<String>,
    pub season: Option<u32>,
    pub next_episode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    pub auto_tag_audio: bool,
    pub auto_tag_video: bool,
    pub allow_online_access: bool,
    pub thediscdb_enabled: bool,
    pub tvmaze_enabled: bool,
    pub tmdb_access_token: Option<String>,
    pub tvdb_api_key: Option<String>,
    pub tvdb_subscriber_pin: Option<String>,
    /// Optional public TVMaze or TheTVDB page selected by the user to bypass
    /// ambiguous title-based video metadata searches.
    pub video_metadata_url: Option<String>,
    /// Used only after authored, fingerprint and provider matching has failed.
    pub previous_dvd_match: Option<PreviousDvdMatch>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            auto_tag_audio: true,
            auto_tag_video: true,
            allow_online_access: true,
            thediscdb_enabled: true,
            tvmaze_enabled: true,
            tmdb_access_token: None,
            tvdb_api_key: None,
            tvdb_subscriber_pin: None,
            video_metadata_url: None,
            previous_dvd_match: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum MediaEngineError {
    #[error("the input does not exist: {0}")]
    MissingInput(PathBuf),
    #[error("the input format is not supported: {0}")]
    UnsupportedInput(String),
    #[error("a required media tool is unavailable: {0}")]
    MissingTool(String),
    #[error("the media process failed: {0}")]
    Process(String),
    #[error("the operation was cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// UI-facing media operations. Implementations must do long work off the GTK thread.
pub trait MediaEngine: Send + Sync {
    fn available_drives(&self) -> Result<Vec<Drive>, MediaEngineError>;
    fn scan(
        &self,
        source: PathBuf,
        options: ScanOptions,
        events: Sender<EngineEvent>,
        cancellation: CancellationToken,
    );
    fn rip(
        &self,
        request: RipRequest,
        events: Sender<EngineEvent>,
        cancellation: CancellationToken,
    );
}
