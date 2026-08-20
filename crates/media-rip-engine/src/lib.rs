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

pub use engine::OpenMediaEngine;
pub use metadata::{EpisodeSeasonOptions, refresh_episode_season, video_metadata_link_provider};
pub use platform::discover_optical_drives;
pub use probe::{ToolAvailability, tool_availability};
