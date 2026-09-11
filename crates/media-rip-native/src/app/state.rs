use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use media_rip_core::{Drive, MediaCatalog, OutputProfile, VideoMetadata, ports::CancellationToken};

pub(crate) use media_rip_core::VideoEpisodeContext as EpisodeMetadataContext;

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

#[derive(Debug, Clone)]
pub(crate) struct VideoMetadataBackup {
    pub name: String,
    pub output_file: String,
    pub tags: Vec<media_rip_core::MediaTag>,
    pub metadata: Option<VideoMetadata>,
    pub tracks: Vec<media_rip_core::Track>,
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
    pub episode_metadata: Option<EpisodeMetadataContext>,
    pub pending_suggestion_season: Option<u32>,
    pub video_metadata_backups: BTreeMap<u32, VideoMetadataBackup>,
    pub reidentification_backup: Option<Box<MediaCatalog>>,
    pub metadata_step_complete: bool,
    pub metadata_skipped: bool,
}

impl AppState {
    pub fn is_busy(&self) -> bool {
        self.busy != BusyState::Idle
    }
}
