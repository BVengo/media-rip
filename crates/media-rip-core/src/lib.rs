//! Stable domain layer shared by the native UI and media backends.

pub mod model;
pub mod ports;
pub mod util;

pub use model::{
    Chapter, CoverArt, DiscKind, Drive, EngineEvent, InfoField, LogLevel, MediaCatalog,
    MediaSource, MediaTag, OutputProfile, RipProgress, RipRequest, RipSelection, RipTitleResult,
    StreamKind, Title, Track, TrackCoverArt,
};
pub use ports::{MediaEngine, MediaEngineError, ScanOptions};
