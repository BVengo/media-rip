use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use gtk::prelude::*;
use media_rip_core::{MediaCatalog, Title, VideoMetadataSuggestion};

use crate::app::EpisodeMetadataContext;

use super::{ContentAction, InfoAction};

#[derive(Debug, Clone)]
pub(crate) struct MetadataPanel {
    pub root: gtk::Box,
    controls: gtk::Box,
    rows: gtk::Box,
    actions: gtk::Box,
}

impl MetadataPanel {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("metadata-workspace");
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        controls.add_css_class("metadata-controls");
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&rows)
            .build();
        scroll.add_css_class("metadata-scroll");
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.add_css_class("metadata-actions");
        root.append(&controls);
        root.append(&scroll);
        root.append(&actions);
        Self {
            root,
            controls,
            rows,
            actions,
        }
    }

    pub fn render(
        &self,
        catalog: Option<&MediaCatalog>,
        episode_context: Option<&EpisodeMetadataContext>,
        on_content_action: Rc<dyn Fn(ContentAction)>,
        on_info_action: Rc<dyn Fn(InfoAction)>,
    ) {
        clear_box(&self.controls);
        clear_box(&self.rows);
        clear_box(&self.actions);
        let Some(catalog) = catalog else {
            self.rows
                .append(&empty_state("Open a source to edit metadata."));
            return;
        };

        let selected_title_ids = catalog
            .titles
            .iter()
            .filter(|title| title.selected)
            .map(|title| title.id)
            .collect::<BTreeSet<_>>();
        let suggestions = catalog
            .video_metadata_suggestions
            .iter()
            .filter(|suggestion| selected_title_ids.contains(&suggestion.title_id))
            .collect::<Vec<_>>();
        render_controls(
            &self.controls,
            &suggestions,
            episode_context,
            on_content_action.clone(),
            on_info_action.clone(),
        );
        let selected = Rc::new(RefCell::new(
            suggestions
                .iter()
                .map(|suggestion| suggestion.title_id)
                .collect::<BTreeSet<_>>(),
        ));
        let suggested_ids = selected.borrow().clone();
        let assignment_ids = episode_assignment_title_ids(catalog);

        for suggestion in &suggestions {
            let current_name = catalog
                .titles
                .iter()
                .find(|title| title.id == suggestion.title_id)
                .map(|title| title.name.as_str())
                .unwrap_or("Unknown title");
            self.rows.append(&suggestion_row(
                suggestion.title_id,
                current_name,
                &suggestion.name,
                suggestion.metadata.attribution.confidence_percent,
                &suggestion.metadata.attribution.provider,
                selected.clone(),
            ));
        }

        if let Some(context) = episode_context.filter(|context| !context.choices.is_empty()) {
            for title in catalog.titles.iter().filter(|title| {
                title.selected
                    && assignment_ids.contains(&title.id)
                    && !suggested_ids.contains(&title.id)
            }) {
                self.rows
                    .append(&assignment_row(title, context, on_info_action.clone()));
            }
        } else if episode_context.is_some()
            && suggestions.is_empty()
            && !selected_title_ids.is_empty()
        {
            self.rows.append(&empty_state(
                "The series was identified, but the season is unknown. Choose a season above and select Update to load its episodes.",
            ));
        } else if suggestions.is_empty() && !selected_title_ids.is_empty() {
            self.rows.append(&empty_state(
                "No episode metadata is available for this source.",
            ));
        }

        if selected_title_ids.is_empty() {
            self.rows.append(&empty_state(
                "Select at least one title before editing metadata.",
            ));
        }

        if !catalog.video_metadata_suggestions.is_empty() {
            self.actions.set_halign(gtk::Align::End);
            let keep = gtk::Button::with_label("Keep current metadata");
            let callback = on_content_action.clone();
            keep.connect_clicked(move |_| callback(ContentAction::SkipMetadata));
            let apply = gtk::Button::with_label(if suggestions.is_empty() {
                "Continue"
            } else {
                "Apply selected changes"
            });
            apply.add_css_class("suggested-action");
            apply.connect_clicked(move |_| {
                on_content_action(ContentAction::ApplySuggestions(
                    selected.borrow().iter().copied().collect(),
                ));
            });
            self.actions.append(&keep);
            self.actions.append(&apply);
        }
    }
}

fn episode_assignment_title_ids(catalog: &MediaCatalog) -> BTreeSet<u32> {
    const MIN_EPISODE_MS: u64 = 20 * 60_000;
    const MAX_EPISODE_MS: u64 = 120 * 60_000;
    const PLAY_ALL_TOLERANCE_MS: u64 = 3_000;

    let episode_like = |title: &Title| {
        (MIN_EPISODE_MS..=MAX_EPISODE_MS).contains(&title.duration_ms) && title.chapters.len() >= 2
    };
    let mut play_all_ids = BTreeSet::new();
    for (play_all_index, play_all) in catalog.titles.iter().enumerate() {
        let mut episode_count = 0;
        let mut total_duration_ms = 0_u64;
        for title in catalog.titles.iter().skip(play_all_index + 1) {
            if !episode_like(title) {
                if episode_count > 0 {
                    break;
                }
                continue;
            }
            episode_count += 1;
            total_duration_ms = total_duration_ms.saturating_add(title.duration_ms);
            if episode_count >= 2
                && total_duration_ms.abs_diff(play_all.duration_ms) <= PLAY_ALL_TOLERANCE_MS
            {
                play_all_ids.insert(play_all.id);
                break;
            }
            if total_duration_ms > play_all.duration_ms.saturating_add(PLAY_ALL_TOLERANCE_MS)
                || episode_count == 8
            {
                break;
            }
        }
    }

    catalog
        .titles
        .iter()
        .filter(|title| episode_like(title) && !play_all_ids.contains(&title.id))
        .map(|title| title.id)
        .collect()
}

fn render_controls(
    controls: &gtk::Box,
    suggestions: &[&VideoMetadataSuggestion],
    context: Option<&EpisodeMetadataContext>,
    on_content_action: Rc<dyn Fn(ContentAction)>,
    on_info_action: Rc<dyn Fn(InfoAction)>,
) {
    let summary = gtk::Label::new(Some(if suggestions.is_empty() {
        "Episode assignments"
    } else {
        "Proposed metadata"
    }));
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.add_css_class("config-label");
    controls.append(&summary);

    let configured_season = suggestions
        .iter()
        .find_map(|suggestion| suggestion.metadata.season)
        .or_else(|| context.and_then(|context| context.metadata.season));
    if context.is_none() && configured_season.is_none() {
        return;
    }
    let season = number_entry(configured_season.unwrap_or(1));
    controls.append(&gtk::Label::new(Some("Season")));
    controls.append(&season);

    let first_episode = suggestions
        .iter()
        .filter_map(|suggestion| suggestion.metadata.episode)
        .min();
    let first = first_episode.map(number_entry);
    if let Some(first) = &first {
        controls.append(&gtk::Label::new(Some("First episode")));
        controls.append(first);
    }
    let has_suggestions = !suggestions.is_empty();
    let update = gtk::Button::with_label("Update");
    update.connect_clicked(move |_| {
        let requested_season = positive_number(&season);
        if requested_season != configured_season
            && let Some(requested_season) = requested_season
        {
            if has_suggestions {
                on_content_action(ContentAction::SetSuggestionSeason(requested_season));
            } else {
                on_info_action(InfoAction::SetEpisodeSeason(requested_season));
            }
            return;
        }
        if let (Some(first), Some(current_first)) = (&first, first_episode)
            && let Some(requested_first) = positive_number(first)
            && requested_first != current_first
        {
            on_content_action(ContentAction::SetSuggestionFirstEpisode(requested_first));
        }
    });
    controls.append(&update);
}

fn suggestion_row(
    title_id: u32,
    current_name: &str,
    proposed_name: &str,
    confidence: u8,
    provider: &str,
    selected: Rc<RefCell<BTreeSet<u32>>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("metadata-review-row");
    let check = gtk::CheckButton::new();
    check.set_active(true);
    check.connect_toggled(move |check| {
        if check.is_active() {
            selected.borrow_mut().insert(title_id);
        } else {
            selected.borrow_mut().remove(&title_id);
        }
    });
    row.append(&check);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    let name = gtk::Label::new(Some(&format!("{current_name}  →  {proposed_name}")));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("row-title");
    labels.append(&name);
    let detail = gtk::Label::new(Some(&format!("{provider} · match score {confidence}/100")));
    detail.set_xalign(0.0);
    detail.add_css_class("row-detail");
    labels.append(&detail);
    row.append(&labels);
    row
}

fn assignment_row(
    title: &Title,
    context: &EpisodeMetadataContext,
    on_action: Rc<dyn Fn(InfoAction)>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("metadata-review-row");
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    let name = gtk::Label::new(Some(&title.name));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("row-title");
    let status_text = title
        .video_metadata
        .as_ref()
        .and_then(|metadata| metadata.episode)
        .map(|episode| format!("Episode {episode} assigned"))
        .unwrap_or_else(|| "No episode assigned".to_owned());
    let status = gtk::Label::new(Some(&status_text));
    status.set_xalign(0.0);
    status.add_css_class("row-detail");
    labels.append(&name);
    labels.append(&status);
    row.append(&labels);

    let names = context
        .choices
        .iter()
        .map(|choice| choice.name.as_str())
        .collect::<Vec<_>>();
    let episodes = context
        .choices
        .iter()
        .map(|choice| choice.episode)
        .collect::<Vec<_>>();
    let chooser = gtk::DropDown::from_strings(&names);
    chooser.set_size_request(300, -1);
    if let Some(current_episode) = title
        .video_metadata
        .as_ref()
        .and_then(|metadata| metadata.episode)
        && let Some(index) = episodes
            .iter()
            .position(|episode| *episode == current_episode)
    {
        chooser.set_selected(index as u32);
    }
    let apply = gtk::Button::with_label(if title.video_metadata.is_some() {
        "Change"
    } else {
        "Assign"
    });
    let title_id = title.id;
    let selected_episode = chooser.clone();
    apply.connect_clicked(move |_| {
        if let Some(episode) = episodes.get(selected_episode.selected() as usize) {
            on_action(InfoAction::AssignEpisode {
                title_id,
                episode: *episode,
            });
        }
    });
    row.append(&chooser);
    row.append(&apply);
    row
}

fn number_entry(value: u32) -> gtk::Entry {
    gtk::Entry::builder()
        .text(value.to_string())
        .width_chars(3)
        .max_width_chars(4)
        .input_purpose(gtk::InputPurpose::Number)
        .build()
}

fn positive_number(entry: &gtk::Entry) -> Option<u32> {
    entry.text().parse::<u32>().ok().filter(|value| *value > 0)
}

fn empty_state(message: &str) -> gtk::Box {
    let state = gtk::Box::new(gtk::Orientation::Vertical, 0);
    state.add_css_class("inline-empty-state");
    state.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(Some(message));
    label.add_css_class("row-detail");
    state.append(&label);
    state
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use media_rip_core::{Chapter, DiscKind, MediaCatalog, MediaSource, Title};

    use super::episode_assignment_title_ids;

    #[test]
    fn manual_episode_candidates_exclude_play_all_and_short_extras() {
        let title = |id, name: &str, duration_ms| Title {
            id,
            source_title_id: id.to_string(),
            disc_title_number: Some(id + 1),
            name: name.to_owned(),
            is_main_title: false,
            duration_ms,
            size_bytes: None,
            size_is_estimate: false,
            chapters: vec![
                Chapter {
                    index: 0,
                    start_ms: 0,
                    end_ms: duration_ms / 2,
                    name: None,
                },
                Chapter {
                    index: 1,
                    start_ms: duration_ms / 2,
                    end_ms: duration_ms,
                    name: None,
                },
            ],
            tracks: Vec::new(),
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            video_metadata: None,
            output_file: format!("{id}.mkv"),
            selected: true,
        };
        let catalog = MediaCatalog {
            source: MediaSource {
                id: "dvd".to_owned(),
                name: "Example Show".to_owned(),
                path: PathBuf::from("dvd"),
                kind: DiscKind::Dvd,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: vec![
                title(0, "Play all", 84 * 60_000),
                title(1, "Episode one", 42 * 60_000),
                title(2, "Episode two", 42 * 60_000),
                title(3, "Extra", 10 * 60_000),
            ],
            source_info: Vec::new(),
            cover_art: None,
            track_cover_art: Default::default(),
            metadata_attributions: Vec::new(),
            video_metadata_suggestions: Vec::new(),
            video_episode_context: None,
        };

        assert_eq!(
            episode_assignment_title_ids(&catalog),
            [1, 2].into_iter().collect()
        );
    }
}
