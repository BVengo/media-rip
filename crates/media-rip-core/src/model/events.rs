use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::MediaCatalog;

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
    ScanFailed(String),
    ScanCancelled,
    RipStarted {
        title_count: usize,
    },
    RipProgress(RipProgress),
    TitleFinished {
        title_name: String,
        destination: PathBuf,
    },
    RipFinished {
        saved_titles: usize,
        destination: PathBuf,
    },
    RipFailed(String),
    RipCancelled,
}
