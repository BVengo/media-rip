//! Authored DVD playback coordinates, independent of guessed film/TV identity.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DvdChapterRange {
    pub first: u32,
    pub last: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DvdTitleInfo {
    pub title_set: u8,
    pub title_in_set: u8,
    pub chapter_range: Option<DvdChapterRange>,
    pub cell_count: u32,
    pub leading_padding_cells: u32,
    pub trailing_padding_cells: u32,
    /// Structural concerns are not proof of copy protection or invalid content.
    pub warnings: Vec<String>,
}
