use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::MediaCatalog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RipSelection {
    pub title_id: u32,
    pub stream_indices: Vec<u32>,
    pub output_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RipRequest {
    pub catalog: MediaCatalog,
    pub selections: Vec<RipSelection>,
    pub output_directory: PathBuf,
    pub overwrite: bool,
}

impl RipRequest {
    pub fn from_catalog(catalog: MediaCatalog, output_directory: PathBuf, overwrite: bool) -> Self {
        let selections = catalog
            .titles
            .iter()
            .filter(|title| title.selected)
            .map(|title| RipSelection {
                title_id: title.id,
                stream_indices: title
                    .tracks
                    .iter()
                    .filter(|track| track.selected)
                    .map(|track| track.index)
                    .collect(),
                output_file: title.output_file.clone(),
            })
            .collect();

        Self {
            catalog,
            selections,
            output_directory,
            overwrite,
        }
    }
}
