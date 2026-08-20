//! Stable domain layer shared by the native UI and media backends.

pub mod model;
pub mod ports;
pub mod util;

pub use model::{
    Chapter, CoverArt, DiscKind, Drive, EngineEvent, InfoField, LogLevel, MediaCatalog,
    MediaSource, MediaTag, MetadataAttribution, OutputProfile, RipProgress, RipRequest,
    RipSelection, RipTitleResult, StreamKind, Title, Track, TrackCoverArt, VideoEpisodeChoice,
    VideoMetadata, VideoMetadataSuggestion,
};
pub use ports::{MediaEngine, MediaEngineError, PreviousDvdMatch, ScanOptions};
