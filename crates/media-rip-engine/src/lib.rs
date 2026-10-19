//! Media probing, remuxing, and encoding engine.
//!
//! This crate intentionally has no dependency on MakeMKV or `makemkvcon`.

mod diagnostics;
mod engine;
mod metadata;
mod platform;
mod probe;
mod process;
mod rip;
mod source;
mod storage;

pub use storage::atomic_write;

pub use engine::OpenMediaEngine;
pub use metadata::{
    EpisodeSeasonOptions, VideoSearchCandidate, VideoSearchKind, identify_loaded_catalog,
    load_video_search_artwork, refresh_episode_season, search_video_metadata,
    video_metadata_link_provider,
};
pub use platform::discover_optical_drives;
pub use probe::{ToolAvailability, tool_availability};

/// Validate a rip without creating directories or starting media processes.
pub fn validate_rip_request(
    request: &media_rip_core::RipRequest,
) -> Result<(), media_rip_core::MediaEngineError> {
    rip::validate_request(request)
}
