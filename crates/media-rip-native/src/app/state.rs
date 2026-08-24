use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use media_rip_core::{
    Drive, MediaCatalog, OutputProfile, VideoEpisodeChoice, VideoMetadata, ports::CancellationToken,
};

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

/// Season metadata retained independently from the pending automatic
/// suggestions. This lets a user assign an episode that the heuristic did not
/// propose, including after all automatic suggestions have been applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EpisodeMetadataContext {
    pub metadata: VideoMetadata,
    pub choices: Vec<VideoEpisodeChoice>,
}

#[derive(Debug, Clone)]
pub(crate) struct VideoMetadataBackup {
    pub name: String,
    pub output_file: String,
    pub tags: Vec<media_rip_core::MediaTag>,
    pub metadata: Option<VideoMetadata>,
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
}

impl AppState {
    pub fn is_busy(&self) -> bool {
        self.busy != BusyState::Idle
    }
}
