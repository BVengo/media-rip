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

/// Mark a dominant DVD feature without guessing film identity from its disc label.
/// Comparable titles and episode collections remain unclassified.
pub fn infer_main_title(catalog: &mut super::MediaCatalog) {
    use super::{DiscKind, metadata::MetadataKind};
    if catalog.source.kind != DiscKind::Dvd
        || catalog.titles.iter().any(|title| title.is_main_title)
        || catalog.video_episode_context.is_some()
        || catalog
            .titles
            .iter()
            .any(|title| MetadataKind::for_title(catalog, title) == MetadataKind::Television)
    {
        return;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut candidates = catalog
        .titles
        .iter()
        .filter(|title| {
            title.dvd.as_ref().is_none_or(|dvd| dvd.warnings.is_empty())
                && title
                    .duplicate_key
                    .as_ref()
                    .is_none_or(|key| seen.insert(key.clone()))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|title| std::cmp::Reverse(title.duration_ms));
    let Some(feature) = candidates.first() else {
        return;
    };
    let other_duration = candidates
        .iter()
        .skip(1)
        .fold(0_u64, |sum, title| sum.saturating_add(title.duration_ms));
    let runner_up_duration = candidates.get(1).map_or(0, |title| title.duration_ms);
    // Forty minutes avoids treating a lone trailer as a feature. Requiring the
    // feature to outweigh all other paths avoids TV play-all compilations.
    if feature.duration_ms < 40 * 60 * 1_000
        || feature.duration_ms < runner_up_duration.saturating_mul(2)
        || feature.duration_ms <= other_duration
    {
        return;
    }
    let id = feature.id;
    catalog
        .titles
        .iter_mut()
        .find(|title| title.id == id)
        .unwrap()
        .is_main_title = true;
}
