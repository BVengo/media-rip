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
    fn scan(&self, source: PathBuf, events: Sender<EngineEvent>, cancellation: CancellationToken);
    fn rip(
        &self,
        request: RipRequest,
        events: Sender<EngineEvent>,
        cancellation: CancellationToken,
    );
}
