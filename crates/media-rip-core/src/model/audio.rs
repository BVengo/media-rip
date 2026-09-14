//! Physical audio CD observations, independent of online metadata providers.
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocTrack {
    pub number: u32,
    pub sectors: u64,
    pub start_sector: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CdText {
    pub album: Option<String>,
    pub artist: Option<String>,
    pub tracks: HashMap<u32, TrackText>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TrackText {
    pub title: Option<String>,
    pub artist: Option<String>,
}
