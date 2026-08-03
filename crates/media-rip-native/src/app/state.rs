use std::{collections::BTreeSet, path::PathBuf};

use media_rip_core::{Drive, MediaCatalog, OutputProfile, ports::CancellationToken};

use super::AppSettings;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Selection {
    #[default]
    Source,
    Title(u32),
    Track {
        title_id: u32,
        stream_index: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum BusyState {
    #[default]
    Idle,
    Scanning,
    Ripping,
}

#[derive(Debug, Default)]
pub(crate) struct AppState {
    pub catalog: Option<MediaCatalog>,
    pub drives: Vec<Drive>,
    pub expanded_titles: BTreeSet<u32>,
    pub selection: Selection,
    pub output_directory: PathBuf,
    pub output_profile: OutputProfile,
    pub settings: AppSettings,
    pub busy: BusyState,
    pub cancellation: Option<CancellationToken>,
    pub completed_titles_this_job: usize,
    pub close_requested: bool,
    pub auto_read_armed: bool,
}

impl AppState {
    pub fn is_busy(&self) -> bool {
        self.busy != BusyState::Idle
    }
}
