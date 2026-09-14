//! Stable domain layer shared by the native UI and media backends.

pub mod model;
pub mod ports;
pub mod util;

pub use model::{
    AudioMetadataCandidate, Chapter, CoverArt, DiscKind, Drive, EngineEvent, InfoField, LogLevel,
    MediaCatalog, MediaSource, MediaTag, MetadataAttribution, OutputProfile, RipPhase, RipProgress,
    RipRequest, RipSelection, RipTitleResult, StreamKind, Title, Track, TrackCoverArt,
    VideoEpisodeChoice, VideoEpisodeContext, VideoMetadata, VideoMetadataSuggestion,
};
pub use ports::{MediaEngine, MediaEngineError, PreviousDvdMatch, ScanOptions};
