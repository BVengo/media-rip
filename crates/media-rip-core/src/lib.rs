//! Stable domain layer shared by the native UI and media backends.

pub mod model;
pub mod ports;
pub mod util;

pub use model::{
    Chapter, DiscKind, Drive, EngineEvent, InfoField, LogLevel, MediaCatalog, MediaSource,
    RipProgress, RipRequest, RipSelection, StreamKind, Title, Track,
};
pub use ports::{MediaEngine, MediaEngineError};
