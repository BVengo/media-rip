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
    pub metadata_request_id: u64,
    pub metadata_cancellation: Option<CancellationToken>,
    pub video_metadata_backups: BTreeMap<u32, VideoMetadataBackup>,
    pub reidentification_backup: Option<Box<MediaCatalog>>,
    pub metadata_step_complete: bool,
    pub metadata_skipped: bool,
}

impl AppState {
    pub fn cancel_metadata_refresh(&mut self) {
        if let Some(token) = self.metadata_cancellation.take() {
            token.cancel();
        }
        self.metadata_request_id = self.metadata_request_id.wrapping_add(1);
        self.pending_suggestion_season = None;
    }

    pub fn begin_metadata_refresh(&mut self, season: u32) -> (u64, CancellationToken) {
        self.cancel_metadata_refresh();
        let token = CancellationToken::default();
        self.metadata_cancellation = Some(token.clone());
        self.pending_suggestion_season = Some(season);
        (self.metadata_request_id, token)
    }

    pub fn accept_metadata_refresh(&mut self, request_id: u64, season: u32) -> bool {
        if self.metadata_request_id != request_id
            || self.pending_suggestion_season != Some(season)
            || self.is_busy()
        {
            return false;
        }
        self.pending_suggestion_season = None;
        self.metadata_cancellation = None;
        true
    }

    pub fn is_busy(&self) -> bool {
        self.busy != BusyState::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_same_season_response_cannot_mutate_a_new_source() {
        let mut state = AppState::default();
        let (old_id, old_token) = state.begin_metadata_refresh(1);
        state.cancel_metadata_refresh();
        let (new_id, _) = state.begin_metadata_refresh(1);
        assert!(old_token.is_cancelled());
        assert!(!state.accept_metadata_refresh(old_id, 1));
        assert!(state.accept_metadata_refresh(new_id, 1));
        assert!(!state.accept_metadata_refresh(new_id, 1));
    }
}
