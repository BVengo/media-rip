use media_rip_core::model::{editing::episode_assignment_title_ids, metadata::MetadataKind};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use gtk::prelude::*;
use media_rip_core::{
    MediaCatalog, Title, VideoEpisodeChoice, VideoMetadataSuggestion, util::format_duration,
};

use crate::app::EpisodeMetadataContext;

use super::{ContentAction, InfoAction};

#[derive(Debug, Clone)]
pub(crate) struct MetadataPanel {
    pub root: gtk::Box,
    controls: gtk::Box,
    rows: gtk::Box,
    actions: gtk::Box,
    assignments: Rc<RefCell<BTreeMap<u32, u32>>>,
    assignment_context: Rc<RefCell<String>>,
}

impl MetadataPanel {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("metadata-workspace");
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        controls.add_css_class("metadata-controls");
        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rows.add_css_class("metadata-rows");
        let scroll = gtk::ScrolledWindow::builder()
            .overlay_scrolling(false)
            .min_content_height(160)
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
            assignments: Rc::default(),
            assignment_context: Rc::default(),
        }
    }

    pub fn set_read_only(&self, read_only: bool) {
        self.controls.set_sensitive(!read_only);
        self.rows.set_sensitive(!read_only);
        self.actions.set_sensitive(!read_only);
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
        self.actions.set_halign(gtk::Align::Fill);
        let Some(catalog) = catalog else {
            self.rows
                .append(&empty_state("Open a source to edit metadata."));
            return;
        };

        if !catalog.audio_metadata_candidates.is_empty() {
            let guidance = detail_note("Choose the release matching your disc packaging.");
            guidance.add_css_class("metadata-intro");
            self.rows.append(&guidance);
            for candidate in &catalog.audio_metadata_candidates {
                let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
                row.add_css_class("metadata-release");
                let heading = gtk::Label::new(Some(&candidate.album));
                heading.set_xalign(0.0);
                heading.set_wrap(true);
                heading.add_css_class("config-label");
                row.append(&heading);
                let label = gtk::Label::new(Some(&format!(
                    "{} · {}\n{}",
                    candidate.artist, candidate.edition, candidate.evidence
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
                apply.set_halign(gtk::Align::Start);
                apply.add_css_class("suggested-action");
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

        let context_key = format!("{}:{:?}", catalog.source.path.display(), episode_context);
        if *self.assignment_context.borrow() != context_key {
            self.assignments.borrow_mut().clear();
            *self.assignment_context.borrow_mut() = context_key;
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
        let assignment_ids = if has_episodes {
            episode_assignment_title_ids(catalog)
        } else {
            BTreeSet::new()
        };

        let assignment_context = episode_context.filter(|context| !context.choices.is_empty());
        let inline_ids = if assignment_context.is_some() {
            assignment_ids
                .into_iter()
                .filter(|id| {
                    catalog.titles.iter().any(|title| {
                        title.id == *id
                            && MetadataKind::for_title(catalog, title) == MetadataKind::Television
                    })
                })
                .collect()
        } else {
            BTreeSet::new()
        };
        self.assignments
            .borrow_mut()
            .retain(|id, _| selected_title_ids.contains(id));
        for suggestion in suggestions
            .iter()
            .filter(|suggestion| inline_ids.contains(&suggestion.title_id))
        {
            if let Some(episode) = suggestion.metadata.episode {
                self.assignments
                    .borrow_mut()
                    .entry(suggestion.title_id)
                    .or_insert(episode);
            }
        }
        if let Some(context) = assignment_context {
            self.render_sequence_controls(catalog, context, &inline_ids, on_info_action.clone());
            self.rows.append(&episode_assignment_table(
                catalog,
                context,
                &inline_ids,
                self.assignments.clone(),
                on_info_action.clone(),
            ));
        }

        let table_suggestions = suggestions
            .iter()
            .copied()
            .filter(|suggestion| !inline_ids.contains(&suggestion.title_id))
            .collect::<Vec<_>>();
        if !table_suggestions.is_empty() {
            let table = gtk::Box::new(gtk::Orientation::Vertical, 0);
            table.add_css_class("metadata-suggestions");
            let columns = SuggestionColumns::new();
            table.append(&columns.header());
            for suggestion in &table_suggestions {
                let title = catalog
                    .titles
                    .iter()
                    .find(|title| title.id == suggestion.title_id);
                table.append(&suggestion_row(
                    suggestion,
                    title,
                    selected.clone(),
                    &columns,
                    title.map(|title| {
                        metadata_editor(
                            title,
                            MetadataKind::for_title(catalog, title),
                            selected_title_ids.len() > 1,
                            false,
                            on_info_action.clone(),
                        )
                    }),
                ));
            }
            self.rows.append(&table);
        }

        for title in catalog.titles.iter().filter(|title| {
            title.selected
                && !inline_ids.contains(&title.id)
                && !table_suggestions
                    .iter()
                    .any(|suggestion| suggestion.title_id == title.id)
        }) {
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
                selected_title_ids.len() == 1 && suggestions.is_empty(),
                on_info_action.clone(),
            );
            if selected_title_ids.len() == 1 && suggestions.is_empty() {
                self.rows.append(&editor);
            } else {
                let expander =
                    gtk::Expander::new(Some(&format!("Edit {} · {}", title.name, kind.label())));
                expander.set_child(Some(&editor));
                expander.add_css_class("metadata-item");
                self.rows.append(&expander);
            }
        }

        if assignment_context.is_none()
            && episode_context.is_some()
            && suggestions.is_empty()
            && !selected_title_ids.is_empty()
        {
            self.rows.append(&empty_state(
                "The series was identified, but the season is unknown. Choose a season above and select Update to load its episodes.",
            ));
        } else if assignment_context.is_none()
            && has_episodes
            && suggestions.is_empty()
            && !selected_title_ids.is_empty()
        {
            self.rows.append(&empty_state(
                "No episode metadata is available for this source.",
            ));
        }

        if selected_title_ids.is_empty() {
            self.rows.append(&empty_state(
                "Select at least one title before editing metadata.",
            ));
        }

        if !inline_ids.is_empty() && table_suggestions.is_empty() && !suggestions.is_empty() {
            let keep = gtk::Button::with_label("Keep current metadata");
            let callback = on_content_action.clone();
            keep.connect_clicked(move |_| callback(ContentAction::SkipMetadata));
            keep.add_css_class("flat");
            self.actions
                .insert_child_after(&keep, self.actions.first_child().as_ref());
        }
        selected.borrow_mut().retain(|id| !inline_ids.contains(id));
        if !catalog.video_metadata_suggestions.is_empty()
            && (inline_ids.is_empty() || !table_suggestions.is_empty())
        {
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
    fn render_sequence_controls(
        &self,
        catalog: &MediaCatalog,
        context: &EpisodeMetadataContext,
        ids: &BTreeSet<u32>,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        let mut titles = catalog
            .titles
            .iter()
            .filter(|title| title.selected && ids.contains(&title.id))
            .collect::<Vec<_>>();
        titles.sort_by_key(|title| (title.disc_title_number.unwrap_or(title.id), title.id));
        let title_ids = titles.iter().map(|title| title.id).collect::<Vec<_>>();
        let title_count = title_ids.len();
        if title_ids.is_empty() {
            return;
        }
        let first_value = titles
            .first()
            .and_then(|title| {
                self.assignments
                    .borrow()
                    .get(&title.id)
                    .copied()
                    .or_else(|| {
                        title
                            .video_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.episode)
                    })
            })
            .unwrap_or(context.choices[0].episode);
        let first = number_entry(first_value);
        first.set_widget_name("sequence-first-episode");
        let sequence = gtk::Button::with_label("Fill in disc order");
        sequence.set_tooltip_text(Some("Preview consecutive episodes for selected episode titles, starting at the first episode. Review before applying."));
        let note = detail_note("");
        note.set_visible(false);
        note.add_css_class("metadata-sequence-note");
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        controls.add_css_class("metadata-sequence-controls");
        controls.append(&gtk::Label::new(Some("Starting episode")));
        controls.append(&first);
        controls.append(&sequence);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_size_request(24, -1);
        self.controls.append(&spacer);
        self.controls.append(&controls);
        self.controls.set_visible(true);
        self.rows.append(&note);
        // Populate dropdowns after all rows have been rendered, without rebuilding them.
        let rows = self.rows.downgrade();
        let choices = context.choices.clone();
        sequence.connect_clicked(move |_| {
            let Some(rows) = rows.upgrade() else { return; };
            let start = first.text().parse::<u32>().ok().and_then(|episode| choices.iter().position(|choice| choice.episode == episode));
            let Some(start) = start.filter(|start| choices.len() - start >= title_ids.len()) else {
                note.set_visible(true);
                note.add_css_class("error");
                note.set_text("Choose a first episode with enough remaining episodes for all selected titles.");
                return;
            };
            for (id, choice) in title_ids.iter().zip(&choices[start..]) {
                if let Some(chooser) = find_assignment_chooser(&rows, *id) {
                    let index = choices.iter().position(|item| item.episode == choice.episode).unwrap();
                    chooser.set_selected(index as u32 + 1);
                }
            }
            note.set_visible(false);
        });
        let summary = detail_note(&format!("{title_count} disc titles"));
        summary.set_hexpand(true);
        self.actions.append(&summary);
        let apply = gtk::Button::with_label("Apply episode metadata");
        apply.add_css_class("suggested-action");
        let drafts = self.assignments.clone();
        let eligible = ids.clone();
        apply.connect_clicked(move |_| {
            let assignments = drafts
                .borrow()
                .iter()
                .filter(|(id, _)| eligible.contains(id))
                .map(|(id, episode)| (*id, *episode))
                .collect::<Vec<_>>();
            if !assignments.is_empty() {
                on_action(InfoAction::AssignEpisodes(assignments));
            }
        });
        self.actions.set_visible(true);
        self.actions.append(&apply);
    }
}

fn metadata_editor(
    title: &Title,
    kind: MetadataKind,
    allow_bulk: bool,
    show_heading: bool,
    on_action: Rc<dyn Fn(InfoAction)>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("metadata-editor");
    let heading = gtk::Label::new(Some(&format!("{} · {}", title.name, kind.label())));
    heading.set_xalign(0.0);
    heading.set_wrap(true);
    heading.add_css_class("config-label");
    if show_heading {
        row.append(&heading);
    }
    if let Some(metadata) = &title.video_metadata {
        let attribution = &metadata.attribution;
        row.append(&detail_note(&format!(
            "{} · match score {}/100 · {}",
            attribution.provider, attribution.confidence_percent, attribution.match_method
        )));
    }
    if title.video_metadata.is_none()
        && !title.tags.iter().any(|tag| {
            matches!(
                tag.key.to_ascii_lowercase().as_str(),
                "media_type" | "show" | "season_number" | "episode_id"
            ) && !tag.value.trim().is_empty()
        })
        && matches!(kind, MetadataKind::Film | MetadataKind::Television)
    {
        row.append(&detail_note("Content type is inferred from the source or playback lengths. Confirm it below; no identity or episode number has been assigned."));
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
            &["Video", "Film", "Television", "Extras"],
            &["video", "film", "television", "extras"],
            match kind {
                MetadataKind::Film => 1,
                MetadataKind::Television => 2,
                MetadataKind::Extras => 3,
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
    if context.is_none_or(|context| context.choices.is_empty()) {
        controls.append(&summary);
    }

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
    let first = first_episode
        .filter(|_| context.is_none_or(|context| context.choices.is_empty()))
        .map(number_entry);
    if let Some(first) = &first {
        controls.append(&gtk::Label::new(Some("First episode")));
        controls.append(first);
    }
    let has_suggestions = !episode_suggestions.is_empty();
    let update = gtk::Button::with_label(
        if context.is_some_and(|context| !context.choices.is_empty()) {
            "Load season"
        } else {
            "Update"
        },
    );
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

struct SuggestionColumns {
    selection: gtk::SizeGroup,
    details: gtk::SizeGroup,
}

impl SuggestionColumns {
    fn new() -> Self {
        Self {
            selection: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            details: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
        }
    }

    fn header(&self) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("metadata-table-heading");
        let selection = gtk::Label::new(Some("Apply"));
        self.selection.add_widget(&selection);
        row.append(&selection);
        row.append(&name_comparison("Current", "Proposed"));
        let spacer = gtk::Label::new(None);
        self.details.add_widget(&spacer);
        row.append(&spacer);
        row
    }
}

fn comparison_cell(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_valign(gtk::Align::Start);
    label.set_hexpand(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_width_chars(10);
    label
}

fn name_comparison(current: &str, proposed: &str) -> gtk::Box {
    let columns = gtk::Box::new(gtk::Orientation::Horizontal, 20);
    columns.set_homogeneous(true);
    columns.set_hexpand(true);
    columns.append(&comparison_cell(current));
    columns.append(&comparison_cell(proposed));
    columns
}

fn suggestion_row(
    suggestion: &VideoMetadataSuggestion,
    title: Option<&Title>,
    selected: Rc<RefCell<BTreeSet<u32>>>,
    columns: &SuggestionColumns,
    editor: Option<gtk::Box>,
) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.add_css_class("metadata-suggestion");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("metadata-review-row");
    let check = gtk::CheckButton::new();
    check.set_active(true);
    check.set_valign(gtk::Align::Start);
    check.update_property(&[gtk::accessible::Property::Label(&format!(
        "Apply proposed metadata for {}",
        suggestion.name
    ))]);
    let title_id = suggestion.title_id;
    check.connect_toggled(move |check| {
        if check.is_active() {
            selected.borrow_mut().insert(title_id);
        } else {
            selected.borrow_mut().remove(&title_id);
        }
    });
    columns.selection.add_widget(&check);
    row.append(&check);
    row.append(&name_comparison(
        title.map_or("Unknown title", |title| title.name.as_str()),
        &suggestion.name,
    ));
    let details = gtk::ToggleButton::builder()
        .icon_name("pan-end-symbolic")
        .has_frame(false)
        .tooltip_text("Edit metadata and review match evidence")
        .valign(gtk::Align::Start)
        .build();
    details.set_widget_name(&format!("metadata-details-{}", suggestion.title_id));
    details.add_css_class("title-expander");
    columns.details.add_widget(&details);
    row.append(&details);
    container.append(&row);
    let body = suggestion_details(suggestion, title);
    if let Some(editor) = editor {
        body.prepend(&editor);
    }
    body.set_visible(false);
    let controlled = body.clone();
    details.connect_toggled(move |button| {
        controlled.set_visible(button.is_active());
        button.set_icon_name(if button.is_active() {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        });
    });
    container.append(&body);
    container
}

fn suggestion_details(suggestion: &VideoMetadataSuggestion, title: Option<&Title>) -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.add_css_class("metadata-suggestion-details");
    let attribution = &suggestion.metadata.attribution;
    body.append(&detail_note(&format!(
        "{} · match score {}/100 · {}",
        attribution.provider, attribution.confidence_percent, attribution.match_method
    )));
    if !suggestion.tags.is_empty() {
        let fields = gtk::Grid::new();
        fields.set_column_spacing(20);
        fields.set_row_spacing(8);
        fields.set_hexpand(true);
        for (column, heading) in ["Field", "Current", "Proposed"].iter().enumerate() {
            let label = comparison_cell(heading);
            label.add_css_class("config-label");
            fields.attach(&label, column as i32, 0, 1, 1);
        }
        for (index, tag) in suggestion.tags.iter().enumerate() {
            let current = title
                .and_then(|title| {
                    title
                        .tags
                        .iter()
                        .find(|current| current.key.eq_ignore_ascii_case(&tag.key))
                })
                .map_or("Not set", |tag| tag.value.as_str());
            for (column, value) in [tag.key.as_str(), current, tag.value.as_str()]
                .iter()
                .enumerate()
            {
                fields.attach(
                    &comparison_cell(value),
                    column as i32,
                    index as i32 + 1,
                    1,
                    1,
                );
            }
        }
        body.append(&fields);
    }
    for detail in &suggestion.evidence {
        body.append(&detail_note(detail));
    }
    for detail in &suggestion.assumptions {
        body.append(&detail_note(&format!("Assumption: {detail}")));
    }
    body
}

fn episode_assignment_table(
    catalog: &MediaCatalog,
    context: &EpisodeMetadataContext,
    ids: &BTreeSet<u32>,
    drafts: Rc<RefCell<BTreeMap<u32, u32>>>,
    on_action: Rc<dyn Fn(InfoAction)>,
) -> gtk::Grid {
    let table = gtk::Grid::new();
    table.set_hexpand(true);
    table.add_css_class("episode-assignment-table");
    for (column, heading) in ["", "Disc title", "Episode", "Episode title"]
        .iter()
        .enumerate()
    {
        let label = gtk::Label::new(Some(heading));
        label.set_xalign(0.0);
        label.add_css_class("episode-column-heading");
        table.attach(&label, column as i32, 0, 1, 1);
    }
    let mut titles = catalog
        .titles
        .iter()
        .filter(|title| title.selected && ids.contains(&title.id))
        .collect::<Vec<_>>();
    titles.sort_by_key(|title| (title.disc_title_number.unwrap_or(title.id), title.id));
    for (index, title) in titles.iter().enumerate() {
        let row = index as i32 * 3 + 1;
        let number = title.disc_title_number.map_or_else(
            || title.source_title_id.clone(),
            |number| number.to_string(),
        );
        let source = gtk::Box::new(gtk::Orientation::Vertical, 3);
        source.set_valign(gtk::Align::Center);
        source.add_css_class("episode-source");
        let name = gtk::Label::new(Some(&format!("Title {number}")));
        name.set_xalign(0.0);
        source.append(&name);
        source.set_tooltip_text(Some(&title.name));
        let length = detail_note(&format_duration(title.duration_ms));
        length.add_css_class("numeric-cell");
        source.append(&length);
        table.attach(&source, 1, row, 1, 1);
        let chooser = episode_chooser(title, context, drafts.clone());
        table.attach(&chooser, 2, row, 1, 1);
        let episode_title = episode_title_label(&chooser);
        episode_title.set_widget_name(&format!("episode-name-{}", title.id));
        episode_title.add_css_class("episode-name");
        table.attach(&episode_title, 3, row, 1, 1);

        let details = episode_manual_details(catalog, title, titles.len() > 1, on_action.clone());
        details.set_visible(false);
        table.attach(&details, 0, row + 1, 4, 1);
        let expand = gtk::ToggleButton::builder()
            .icon_name("pan-end-symbolic")
            .has_frame(false)
            .tooltip_text(format!(
                "Edit metadata and view match details for Title {number}"
            ))
            .valign(gtk::Align::Center)
            .build();
        expand.set_widget_name(&format!("episode-details-{}", title.id));
        expand.add_css_class("title-expander");
        expand.connect_toggled(move |button| {
            details.set_visible(button.is_active());
            button.set_icon_name(if button.is_active() {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            });
        });
        table.attach(&expand, 0, row, 1, 1);
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.add_css_class("episode-row-separator");
        table.attach(&separator, 0, row + 2, 4, 1);
    }
    table
}

fn episode_title_label(chooser: &gtk::DropDown) -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_width_chars(20);
    label.set_hexpand(true);
    let update = |chooser: &gtk::DropDown, label: &gtk::Label| {
        let text = chooser
            .selected_item()
            .and_downcast::<gtk::StringObject>()
            .map(|item| item.string().to_string())
            .unwrap_or_default();
        label.set_text(
            text.split_once(" · ")
                .map_or("Choose an episode", |(_, title)| title),
        );
    };
    update(chooser, &label);
    let weak_label = label.downgrade();
    chooser.connect_selected_notify(move |chooser| {
        if let Some(label) = weak_label.upgrade() {
            update(chooser, &label);
        }
    });
    label
}

fn episode_manual_details(
    catalog: &MediaCatalog,
    title: &Title,
    multiple_titles: bool,
    on_action: Rc<dyn Fn(InfoAction)>,
) -> gtk::Box {
    let editor = metadata_editor(
        title,
        MetadataKind::Television,
        multiple_titles,
        false,
        on_action,
    );
    editor.add_css_class("episode-details");
    editor.set_widget_name(&format!("episode-editor-{}", title.id));
    if let Some(suggestion) = catalog
        .video_metadata_suggestions
        .iter()
        .find(|suggestion| suggestion.title_id == title.id)
    {
        editor.append(&suggestion_details(suggestion, Some(title)));
    }
    editor
}

fn episode_choice_label(choice: &VideoEpisodeChoice, season: Option<u32>) -> String {
    let code = season.map(|season| format!("S{season:02}E{:02}", choice.episode));
    let name = choice
        .tags
        .iter()
        .find(|tag| tag.key.eq_ignore_ascii_case("title"))
        .map(|tag| tag.value.as_str())
        .or_else(|| {
            code.as_ref().and_then(|code| {
                choice
                    .name
                    .split_once(&format!("{code} - "))
                    .map(|(_, name)| name)
            })
        })
        .unwrap_or(&choice.name);
    format!("{:02} · {name}", choice.episode)
}

fn episode_label_factory(compact: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_wrap(!compact);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_width_chars(if compact { 3 } else { 24 });
        label.set_hexpand(true);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = item.child().and_downcast::<gtk::Label>().unwrap();
        let text = item.item().and_downcast::<gtk::StringObject>().unwrap();
        let text = text.string();
        label.set_text(if compact {
            text.split(" · ")
                .next()
                .filter(|text| text.chars().all(|character| character.is_ascii_digit()))
                .unwrap_or("–")
        } else {
            &text
        });
    });
    factory
}

fn episode_chooser(
    title: &Title,
    context: &EpisodeMetadataContext,
    drafts: Rc<RefCell<BTreeMap<u32, u32>>>,
) -> gtk::DropDown {
    let mut names = vec!["Choose an episode…".to_owned()];
    names.extend(
        context
            .choices
            .iter()
            .map(|choice| episode_choice_label(choice, context.metadata.season)),
    );
    let names = names.iter().map(String::as_str).collect::<Vec<_>>();
    let episodes = context
        .choices
        .iter()
        .map(|choice| choice.episode)
        .collect::<Vec<_>>();
    let chooser = gtk::DropDown::from_strings(&names);
    chooser.set_factory(Some(&episode_label_factory(true)));
    chooser.set_list_factory(Some(&episode_label_factory(false)));
    chooser.set_hexpand(false);
    chooser.set_halign(gtk::Align::Start);
    chooser.set_valign(gtk::Align::Center);
    chooser.add_css_class("episode-chooser");
    chooser.set_widget_name(&format!("episode-assignment-{}", title.id));
    chooser.update_property(&[gtk::accessible::Property::Label(&format!(
        "Episode for disc title {}",
        title.disc_title_number.unwrap_or(title.id)
    ))]);
    if let Some(current_episode) = drafts.borrow().get(&title.id).copied().or_else(|| {
        title
            .video_metadata
            .as_ref()
            .and_then(|metadata| metadata.episode)
    }) && let Some(index) = episodes
        .iter()
        .position(|episode| *episode == current_episode)
    {
        chooser.set_selected(index as u32 + 1);
    }
    let title_id = title.id;
    chooser.connect_selected_notify(move |chooser| {
        let mut drafts = drafts.borrow_mut();
        if let Some(episode) = chooser
            .selected()
            .checked_sub(1)
            .and_then(|index| episodes.get(index as usize))
        {
            drafts.insert(title_id, *episode);
        } else {
            drafts.remove(&title_id);
        }
    });
    chooser
}

fn find_assignment_chooser(root: &impl IsA<gtk::Widget>, title_id: u32) -> Option<gtk::DropDown> {
    let widget = root.as_ref();
    if widget.widget_name() == format!("episode-assignment-{title_id}") {
        return widget.clone().downcast().ok();
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(chooser) = find_assignment_chooser(&widget, title_id) {
            return Some(chooser);
        }
        child = widget.next_sibling();
    }
    None
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
