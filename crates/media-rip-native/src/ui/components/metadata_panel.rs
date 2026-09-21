use media_rip_core::model::{editing::episode_assignment_title_ids, metadata::MetadataKind};
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
        self.controls.set_visible(false);
        clear_box(&self.rows);
        clear_box(&self.actions);
        self.actions.set_visible(false);
        let Some(catalog) = catalog else {
            self.rows
                .append(&empty_state("Open a source to edit metadata."));
            return;
        };

        if !catalog.audio_metadata_candidates.is_empty() {
            self.rows.append(&empty_state("Choose the release matching your disc packaging. Existing metadata is kept until you choose."));
            for candidate in &catalog.audio_metadata_candidates {
                let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
                let label = gtk::Label::new(Some(&format!(
                    "{} · {}\n{}\n{}",
                    candidate.album, candidate.artist, candidate.edition, candidate.evidence
                )));
                label.set_wrap(true);
                label.set_xalign(0.0);
                row.append(&label);
                let tracks = candidate
                    .tracks
                    .iter()
                    .map(|(number, tags)| {
                        format!(
                            "{number}. {}",
                            tags.iter()
                                .find(|tag| tag.key == "title")
                                .map(|tag| tag.value.as_str())
                                .unwrap_or("Untitled")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let details = gtk::Expander::new(Some("Track list"));
                let track_label = gtk::Label::new(Some(&tracks));
                track_label.set_xalign(0.0);
                details.set_child(Some(&track_label));
                row.append(&details);
                let apply = gtk::Button::with_label("Use this release");
                let id = candidate.id.clone();
                let callback = on_content_action.clone();
                apply.connect_clicked(move |_| {
                    callback(ContentAction::ApplyAudioCandidate(id.clone()))
                });
                row.append(&apply);
                self.rows.append(&row);
            }
            let keep = gtk::Button::with_label("Keep current metadata");
            keep.connect_clicked(move |_| on_content_action(ContentAction::SkipMetadata));
            self.actions.set_visible(true);
            self.actions.append(&keep);
            return;
        }

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
        let has_episodes = catalog.titles.iter().any(|title| {
            title.selected && MetadataKind::for_title(catalog, title) == MetadataKind::Television
        });
        let episode_context = episode_context.filter(|_| has_episodes);
        render_controls(
            &self.controls,
            &suggestions,
            episode_context,
            has_episodes,
            on_content_action.clone(),
            on_info_action.clone(),
        );
        let audio_only = !catalog.titles.is_empty()
            && catalog.titles.iter().all(|title| {
                matches!(
                    MetadataKind::for_title(catalog, title),
                    MetadataKind::Music | MetadataKind::Audiobook
                )
            });
        if audio_only
            && catalog.titles.len() > 1
            && let Some(title) = catalog.titles.first()
        {
            let chooser = gtk::DropDown::from_strings(&["Music", "Audiobook", "Mixed audio"]);
            let first_kind = MetadataKind::for_title(catalog, title);
            let uniform = catalog
                .titles
                .iter()
                .all(|title| MetadataKind::for_title(catalog, title) == first_kind);
            chooser.set_selected(if uniform {
                u32::from(first_kind == MetadataKind::Audiobook)
            } else {
                2
            });
            chooser.set_tooltip_text(Some(
                "Set the metadata type for every track on this source.",
            ));
            let callback = on_info_action.clone();
            let title_id = title.id;
            chooser.connect_selected_notify(move |chooser| {
                let value = match chooser.selected() {
                    0 => "music",
                    1 => "audiobook",
                    _ => return,
                };
                callback(InfoAction::AddTag {
                    title_id,
                    key: "media_type".to_owned(),
                    value: value.to_owned(),
                    all_tracks: true,
                });
            });
            self.controls.append(&gtk::Label::new(Some("All tracks")));
            self.controls.append(&chooser);
        }
        self.controls.set_visible(
            !suggestions.is_empty()
                || self
                    .controls
                    .first_child()
                    .is_some_and(|child| child.next_sibling().is_some()),
        );
        let selected = Rc::new(RefCell::new(
            suggestions
                .iter()
                .map(|suggestion| suggestion.title_id)
                .collect::<BTreeSet<_>>(),
        ));
        let suggested_ids = selected.borrow().clone();
        let assignment_ids = if has_episodes {
            episode_assignment_title_ids(catalog)
        } else {
            BTreeSet::new()
        };

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
            let details = gtk::Expander::new(Some("Review fields and match evidence"));
            let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
            for tag in &suggestion.tags {
                body.append(&detail_note(&format!("{}: {}", tag.key, tag.value)));
            }
            for detail in &suggestion.evidence {
                body.append(&detail_note(&format!("Evidence: {detail}")));
            }
            for detail in &suggestion.assumptions {
                body.append(&detail_note(&format!("Assumption: {detail}")));
            }
            details.set_child(Some(&body));
            self.rows.append(&details);
        }

        for title in catalog.titles.iter().filter(|title| title.selected) {
            let kind = MetadataKind::for_title(catalog, title);
            let editor = metadata_editor(
                title,
                kind,
                catalog
                    .titles
                    .iter()
                    .filter(|other| {
                        other.selected && MetadataKind::for_title(catalog, other) == kind
                    })
                    .count()
                    > 1,
                on_info_action.clone(),
            );
            if selected_title_ids.len() == 1 {
                self.rows.append(&editor);
            } else {
                let expander =
                    gtk::Expander::new(Some(&format!("{} · {}", title.name, kind.label())));
                expander.set_child(Some(&editor));
                expander.add_css_class("metadata-item");
                self.rows.append(&expander);
            }
        }

        if let Some(context) = episode_context.filter(|context| !context.choices.is_empty()) {
            for title in catalog.titles.iter().filter(|title| {
                title.selected
                    && MetadataKind::for_title(catalog, title) == MetadataKind::Television
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
        } else if has_episodes && suggestions.is_empty() && !selected_title_ids.is_empty() {
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
            self.actions.set_visible(true);
            self.actions.append(&keep);
            self.actions.append(&apply);
        }
    }
}

fn metadata_editor(
    title: &Title,
    kind: MetadataKind,
    allow_bulk: bool,
    on_action: Rc<dyn Fn(InfoAction)>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("metadata-editor");
    let heading = gtk::Label::new(Some(&format!("{} · {}", title.name, kind.label())));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    heading.add_css_class("config-label");
    row.append(&heading);
    if let Some(metadata) = &title.video_metadata {
        let attribution = &metadata.attribution;
        row.append(&detail_note(&format!(
            "{} · match score {}/100 · {}",
            attribution.provider, attribution.confidence_percent, attribution.match_method
        )));
    }
    let audio = matches!(kind, MetadataKind::Music | MetadataKind::Audiobook);
    let (labels, values, active): (&[&str], &[&str], u32) = if audio {
        (
            &["Music", "Audiobook"],
            &["music", "audiobook"],
            u32::from(kind == MetadataKind::Audiobook),
        )
    } else {
        (
            &["Video / extras", "Film", "Television"],
            &["video", "film", "television"],
            match kind {
                MetadataKind::Film => 1,
                MetadataKind::Television => 2,
                _ => 0,
            },
        )
    };
    let chooser = gtk::DropDown::from_strings(labels);
    chooser.set_selected(active);
    chooser.set_tooltip_text(Some(
        "Choose the content type. Existing tags are preserved.",
    ));
    let callback = on_action.clone();
    let title_id = title.id;
    chooser.connect_selected_notify(move |chooser| {
        if let Some(value) = values.get(chooser.selected() as usize) {
            callback(InfoAction::AddTag {
                title_id,
                key: "media_type".to_owned(),
                value: (*value).to_owned(),
                all_tracks: false,
            });
        }
    });
    let type_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let type_label = gtk::Label::new(Some("Content type"));
    type_label.set_xalign(0.0);
    type_label.set_width_chars(16);
    type_row.append(&type_label);
    type_row.append(&chooser);
    row.append(&type_row);
    let fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let additional_fields = gtk::Box::new(gtk::Orientation::Vertical, 8);
    for &(key, label) in kind.fields() {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::new(Some(label));
        label.set_width_chars(16);
        label.set_xalign(0.0);
        let entry = gtk::Entry::new();
        entry.set_hexpand(true);
        entry.set_tooltip_text(Some("Changes are saved as you type."));
        let structured_value = title
            .video_metadata
            .as_ref()
            .and_then(|metadata| match key {
                "season_number" => metadata.season,
                "episode_id" => metadata.episode,
                "date" => metadata.year,
                _ => None,
            })
            .map(|value| value.to_string())
            .unwrap_or_default();
        entry.set_text(
            title
                .tags
                .iter()
                .find(|tag| tag.key.eq_ignore_ascii_case(key))
                .map(|tag| tag.value.as_str())
                .unwrap_or(if key == "title" {
                    &title.name
                } else {
                    &structured_value
                }),
        );
        let callback = on_action.clone();
        let title_id = title.id;
        entry.connect_changed(move |entry| {
            if matches!(key, "season_number" | "episode_id") {
                let valid = entry.text().trim().is_empty() || entry.text().parse::<u32>().is_ok();
                if !valid {
                    entry.add_css_class("error");
                    entry.set_tooltip_text(Some(
                        "Enter a whole number, or leave blank. Season 0 is for specials.",
                    ));
                    return;
                }
                entry.remove_css_class("error");
                entry.set_tooltip_text(Some("Changes are saved as you type."));
            }
            callback(InfoAction::SetTag {
                title_id,
                key: key.to_owned(),
                value: entry.text().to_string(),
                all_tracks: false,
            })
        });
        line.append(&label);
        line.append(&entry);
        if allow_bulk
            && matches!(
                key,
                "album"
                    | "album_artist"
                    | "author"
                    | "narrator"
                    | "discnumber"
                    | "date"
                    | "genre"
                    | "show"
                    | "season_number"
                    | "series"
                    | "series-part"
            )
        {
            let apply = gtk::Button::with_label("Apply to selected");
            apply.set_tooltip_text(Some("Apply to selected items of the same content type."));
            let callback = on_action.clone();
            apply.connect_clicked(move |_| {
                if entry.has_css_class("error") {
                    return;
                }
                callback(InfoAction::SetSharedTag {
                    title_id,
                    key: key.to_owned(),
                    value: entry.text().to_string(),
                })
            });
            line.append(&apply);
        }
        if audio
            && matches!(
                key,
                "track" | "discnumber" | "date" | "genre" | "series" | "series-part" | "comment"
            )
        {
            additional_fields.append(&line);
        } else {
            fields.append(&line);
        }
    }
    row.append(&fields);
    if additional_fields.first_child().is_some() {
        let details = gtk::Expander::new(Some("Numbering and release details"));
        details.set_child(Some(&additional_fields));
        row.append(&details);
    }
    row
}

fn render_controls(
    controls: &gtk::Box,
    suggestions: &[&VideoMetadataSuggestion],
    context: Option<&EpisodeMetadataContext>,
    has_episodes: bool,
    on_content_action: Rc<dyn Fn(ContentAction)>,
    on_info_action: Rc<dyn Fn(InfoAction)>,
) {
    let summary = gtk::Label::new(Some(if suggestions.is_empty() {
        if has_episodes {
            "Television metadata"
        } else {
            "Edit metadata"
        }
    } else {
        "Proposed metadata"
    }));
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.add_css_class("config-label");
    controls.append(&summary);

    let episode_suggestions = suggestions
        .iter()
        .copied()
        .filter(|suggestion| {
            MetadataKind::from_video(&suggestion.metadata) == Some(MetadataKind::Television)
        })
        .collect::<Vec<_>>();
    let configured_season = episode_suggestions
        .iter()
        .find_map(|suggestion| suggestion.metadata.season)
        .or_else(|| context.and_then(|context| context.metadata.season));
    if !has_episodes || (context.is_none() && configured_season.is_none()) {
        return;
    }
    let season = number_entry(configured_season.unwrap_or(1));
    controls.append(&gtk::Label::new(Some("Season")));
    controls.append(&season);

    let first_episode = episode_suggestions
        .iter()
        .filter_map(|suggestion| suggestion.metadata.episode)
        .min();
    let first = first_episode.map(number_entry);
    if let Some(first) = &first {
        controls.append(&gtk::Label::new(Some("First episode")));
        controls.append(first);
    }
    let has_suggestions = !episode_suggestions.is_empty();
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

fn detail_note(message: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(message));
    label.add_css_class("row-detail");
    label.set_wrap(true);
    label.set_xalign(0.0);
    label
}

fn empty_state(message: &str) -> gtk::Box {
    let state = gtk::Box::new(gtk::Orientation::Vertical, 0);
    state.add_css_class("inline-empty-state");
    state.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(Some(message));
    label.add_css_class("row-detail");
    label.set_wrap(true);
    label.set_xalign(0.0);
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
            dvd: None,
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
            audio_metadata_candidates: Vec::new(),
            video_metadata_suggestions: Vec::new(),
            video_episode_context: None,
        };

        assert_eq!(
            episode_assignment_title_ids(&catalog),
            [1, 2].into_iter().collect()
        );
    }
}
