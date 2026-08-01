use std::{path::PathBuf, sync::mpsc::Sender};

use media_rip_core::{
    Drive, EngineEvent, MediaEngine, MediaEngineError, RipRequest, ScanOptions,
    ports::CancellationToken,
};

use crate::{platform, probe, rip};

#[derive(Debug, Default)]
pub struct OpenMediaEngine;

impl OpenMediaEngine {
    pub fn new() -> Self {
        Self
    }
}

impl MediaEngine for OpenMediaEngine {
    fn available_drives(&self) -> Result<Vec<Drive>, MediaEngineError> {
        platform::discover_optical_drives()
    }

    fn scan(
        &self,
        source: PathBuf,
        options: ScanOptions,
        events: Sender<EngineEvent>,
        cancellation: CancellationToken,
    ) {
        let _ = events.send(EngineEvent::ScanStarted {
            source: source.clone(),
        });

        match probe::scan_source(&source, options, &events, &cancellation) {
            Ok(catalog) => {
                let _ = events.send(EngineEvent::ScanFinished(Box::new(catalog)));
            }
            Err(MediaEngineError::Cancelled) => {
                let _ = events.send(EngineEvent::Log {
                    level: media_rip_core::LogLevel::Warning,
                    message: "Source scan cancelled.".to_owned(),
                });
                let _ = events.send(EngineEvent::ScanCancelled);
            }
            Err(error) => {
                let _ = events.send(EngineEvent::ScanFailed(error.to_string()));
            }
        }
    }

    fn rip(
        &self,
        request: RipRequest,
        events: Sender<EngineEvent>,
        cancellation: CancellationToken,
    ) {
        match rip::run(request, &events, &cancellation) {
            Ok(()) => {}
            Err(MediaEngineError::Cancelled) => {
                let _ = events.send(EngineEvent::RipCancelled);
            }
            Err(error) => {
                let _ = events.send(EngineEvent::RipFailed(error.to_string()));
            }
        }
    }
}
