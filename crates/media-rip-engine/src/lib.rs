//! Media probing and stream-copy engine.
//!
//! This crate intentionally has no dependency on MakeMKV or `makemkvcon`.

mod engine;
mod platform;
mod probe;
mod process;
mod rip;
mod source;

pub use engine::OpenMediaEngine;
pub use platform::discover_optical_drives;
pub use probe::{ToolAvailability, tool_availability};
