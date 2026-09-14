use std::rc::Rc;
use std::{collections::BTreeSet, io::Cursor};

use gtk::prelude::*;
use media_rip_core::{
    CoverArt, DiscKind, InfoField, MediaCatalog, VideoMetadata,
    util::{format_bytes, format_duration},
};

use crate::app::{EpisodeMetadataContext, Selection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InfoAction {
    AssignEpisode {
        title_id: u32,
        episode: u32,
    },
    SetEpisodeSeason(u32),
    RemoveVideoMetadata {
        title_id: u32,
    },
    SetTitleName {
        title_id: u32,
        name: String,
    },
    SetTrackName {
        title_id: u32,
        stream_index: u32,
        name: String,
    },
    SetTag {
        title_id: u32,
        key: String,
        value: String,
        all_tracks: bool,
    },
    AddTag {
        title_id: u32,
        key: String,
        value: String,
        all_tracks: bool,
    },
    SetOutputFile {
        title_id: u32,
        output_file: String,
    },
    ChooseCoverArt {
        title_id: Option<u32>,
    },
    RemoveCoverArt {
        title_id: Option<u32>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct InfoPanel {
    stack: gtk::Stack,
    tags_page: gtk::StackPage,
    metadata_page: gtk::StackPage,
    info_grid: gtk::Grid,
    info_artwork: gtk::Box,
    tags_content: gtk::Box,
    metadata_content: gtk::Box,
}

impl InfoPanel {
    pub fn new() -> Self {
        let info_grid = gtk::Grid::builder()
            .column_spacing(14)
            .row_spacing(6)
            .valign(gtk::Align::Start)
            .hexpand(true)
            .vexpand(false)
            .build();
        info_grid.add_css_class("info-grid");
        let info_row = gtk::Grid::builder().column_spacing(12).build();
        let info_artwork = gtk::Box::new(gtk::Orientation::Vertical, 0);
        info_artwork.set_valign(gtk::Align::Start);
        info_row.attach(&info_grid, 0, 0, 1, 1);
        info_row.attach(&info_artwork, 1, 0, 1, 1);
        let info_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(72)
            .hexpand(true)
            .vexpand(true)
            .child(&info_row)
            .build();

        let tags_content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        tags_content.add_css_class("tags-content");
        let tags_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(72)
            .hexpand(true)
            .vexpand(true)
            .child(&tags_content)
            .build();

        let metadata_content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        metadata_content.add_css_class("tags-content");
        let metadata_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(72)
            .hexpand(true)
            .vexpand(true)
            .child(&metadata_content)
            .build();

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .hexpand(true)
            .vexpand(true)
            .build();
        stack.add_titled(&info_scroll, Some("info"), "Info");
        let metadata_page = stack.add_titled(&metadata_scroll, Some("metadata"), "Metadata");
        metadata_page.set_visible(false);
        let tags_page = stack.add_titled(&tags_scroll, Some("tags"), "Tags");
        tags_page.set_visible(false);
        let switcher = gtk::StackSwitcher::builder()
            .stack(&stack)
            .halign(gtk::Align::Fill)
            .hexpand(true)
            .build();
        if let Some(layout) = switcher
            .layout_manager()
            .and_then(|layout| layout.downcast::<gtk::BoxLayout>().ok())
        {
            layout.set_homogeneous(true);
        }
        switcher.add_css_class("panel-tabs");

        let panel = Self {
            stack,
            tags_page,
            metadata_page,
            info_grid,
            info_artwork,
            tags_content,
            metadata_content,
        };
        panel.render_fields(&[
            InfoField::new("Type", "No source"),
            InfoField::new("Name", "Insert or open a disc to begin"),
        ]);
        panel.render_tags_unavailable("Tags are available for Audio CD tracks.");
        panel.render_metadata_unavailable("Open a video disc to inspect metadata proposals.");
        panel
    }

    pub fn render(
        &self,
        catalog: Option<&MediaCatalog>,
        selection: &Selection,
        episode_metadata: Option<&EpisodeMetadataContext>,
        can_undo_metadata: bool,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        let tags_available =
            catalog.is_some_and(|catalog| catalog.source.kind == DiscKind::AudioCd);
        let metadata_available = catalog
            .is_some_and(|catalog| matches!(catalog.source.kind, DiscKind::Dvd | DiscKind::BluRay));
        self.tags_page.set_visible(tags_available);
        self.metadata_page.set_visible(metadata_available);
        if (!tags_available && self.stack.visible_child_name().as_deref() == Some("tags"))
            || (!metadata_available
                && self.stack.visible_child_name().as_deref() == Some("metadata"))
        {
            self.stack.set_visible_child_name("info");
        }
        let Some(catalog) = catalog else {
            self.render_fields(&[
                InfoField::new("Type", "No source"),
                InfoField::new("Name", "Insert or open a disc to begin"),
            ]);
            self.render_tags_unavailable("Open an Audio CD and select a track to edit its tags.");
            self.render_metadata_unavailable("Open a video disc to inspect metadata proposals.");
            return;
        };
        self.render_metadata(
            catalog,
            selection,
            episode_metadata,
            can_undo_metadata,
            on_action.clone(),
        );

        match selection {
            Selection::Source => {
                self.render_fields(&catalog.source_info);
                self.append_readonly_artwork(catalog.cover_art.as_ref(), "Disc artwork");
                self.render_tags_unavailable("Select an Audio CD track to edit its tags.");
            }
            Selection::Title(id) => {
                let Some(title) = catalog.titles.iter().find(|title| title.id == *id) else {
                    self.render_fields(&catalog.source_info);
                    self.render_tags_unavailable("Select an Audio CD track to edit its tags.");
                    return;
                };
                let mut fields = vec![
                    InfoField::new("Type", "Media item"),
                    InfoField::new("Source item ID", &title.source_title_id),
                    InfoField::new("Duration", format_duration(title.duration_ms)),
                    InfoField::new("Chapters", title.chapters.len().to_string()),
                ];
                if let Some(number) = title.disc_title_number {
                    fields.push(InfoField::new("Authored title number", number.to_string()));
                }
                if title.is_main_title {
                    fields.push(InfoField::new(
                        "Blu-ray role",
                        "Main title (identified by disc metadata)",
                    ));
                }
                if let Some(original) = catalog
                    .titles
                    .iter()
                    .take_while(|item| item.id != title.id)
                    .find(|item| {
                        item.duplicate_key.is_some() && item.duplicate_key == title.duplicate_key
                    })
                {
                    fields.push(InfoField::new(
                        "Exact duplicate of",
                        format!("Title {}", original.source_title_id),
                    ));
                }
                if let Some(size) = title.size_bytes {
                    fields.push(InfoField::new(
                        if title.size_is_estimate {
                            "Estimated size"
                        } else {
                            "Size"
                        },
                        format_bytes(size),
                    ));
                }
                if let Some(dvd) = &title.dvd {
                    fields.push(InfoField::new(
                        "DVD location",
                        format!("VTS {:02}, title {}", dvd.title_set, dvd.title_in_set),
                    ));
                    if let Some(range) = dvd.chapter_range {
                        fields.push(InfoField::new(
                            "Original DVD chapters",
                            format!("{} to {}", range.first, range.last),
                        ));
                    }
                    fields.push(InfoField::new(
                        "Padding cells",
                        format!(
                            "{} at start, {} at end",
                            dvd.leading_padding_cells, dvd.trailing_padding_cells
                        ),
                    ));
                    for warning in &dvd.warnings {
                        fields.push(InfoField::new("DVD layout review", warning));
                    }
                    fields.push(InfoField::new(
                        "Size basis",
                        if dvd.cell_count > 0 {
                            "Estimated from title cell sectors; not a measured output file size"
                        } else {
                            "DVD layout unverified; any displayed size is an estimate"
                        },
                    ));
                }
                if let Some(count) = title.segment_count {
                    fields.push(InfoField::new("Segment count", count.to_string()));
                }
                if let Some(map) = &title.segment_map {
                    fields.push(InfoField::new("Segment map", map));
                }
                if let Some(ranges) = title
                    .duplicate_key
                    .as_deref()
                    .and_then(format_playlist_ranges)
                {
                    fields.push(InfoField::new("Exact clip ranges", ranges));
                }
                fields.push(InfoField::new(
                    "Selected tracks",
                    title.selected_track_count().to_string(),
                ));
                self.render_fields(&fields);
                self.append_title_name_editor(
                    fields.len(),
                    title.id,
                    &title.name,
                    on_action.clone(),
                );
                if catalog.source.kind == media_rip_core::DiscKind::AudioCd {
                    let tag_keys = tag_key_suggestions(catalog);
                    self.append_track_artwork(
                        catalog.cover_art_for_title(title.id),
                        title.id,
                        on_action.clone(),
                    );
                    self.append_output_file_editor(
                        fields.len() + 1,
                        title.id,
                        &title.output_file,
                        on_action.clone(),
                    );
                    self.render_editable_tags(
                        title.id,
                        &title.tags,
                        &tag_keys,
                        catalog.cover_art.as_ref(),
                        on_action,
                    );
                } else {
                    self.append_readonly_artwork(catalog.cover_art.as_ref(), "Disc artwork");
                    self.append_output_file_editor(
                        fields.len() + 1,
                        title.id,
                        &title.output_file,
                        on_action,
                    );
                    self.render_tags_unavailable("Tags are available for Audio CD tracks.");
                }
            }
            Selection::Track {
                title_id,
                stream_index,
            } => {
                let track = catalog
                    .titles
                    .iter()
                    .find(|title| title.id == *title_id)
                    .and_then(|title| {
                        title
                            .tracks
                            .iter()
                            .find(|track| track.index == *stream_index)
                    });
                let Some(track) = track else {
                    self.render_fields(&catalog.source_info);
                    self.render_tags_unavailable("Select an Audio CD track to edit its tags.");
                    return;
                };
                let mut fields = vec![
                    InfoField::new("Type", track.kind.display_name()),
                    InfoField::new("Codec", &track.codec),
                    InfoField::new("Stream ID", track.index.to_string()),
                ];
                if let Some(description) = &track.codec_long_name {
                    fields.push(InfoField::new("Codec description", description));
                }
                if let Some(language) = &track.language {
                    fields.push(InfoField::new("Language", language));
                }
                if !track.properties.is_empty() {
                    fields.push(InfoField::new("Properties", track.properties.join(", ")));
                }
                self.render_fields(&fields);
                self.append_track_name_editor(
                    fields.len(),
                    *title_id,
                    *stream_index,
                    track.name.as_deref().unwrap_or_default(),
                    on_action,
                );
                self.render_tags_unavailable("Select the Audio CD track row to edit its tags.");
            }
        }
    }

    fn render_fields(&self, fields: &[InfoField]) {
        self.remove_info_artwork();
        clear(&self.info_grid);
        for (row, field) in fields.iter().enumerate() {
            let label = gtk::Label::new(Some(&field.label));
            label.set_xalign(0.0);
            label.set_yalign(0.0);
            label.add_css_class("info-label");
            let value = gtk::Label::new(Some(&field.value));
            value.set_xalign(0.0);
            value.set_yalign(0.0);
            value.set_wrap(true);
            value.set_selectable(true);
            value.set_hexpand(true);
            value.add_css_class("info-value");
            self.info_grid.attach(&label, 0, row as i32, 1, 1);
            self.info_grid.attach(&value, 1, row as i32, 1, 1);
        }
    }

    fn render_editable_tags(
        &self,
        title_id: u32,
        tags: &[media_rip_core::MediaTag],
        tag_keys: &[String],
        cover_art: Option<&CoverArt>,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        clear(&self.tags_content);

        self.tags_content
            .append(&cover_art_editor(cover_art, on_action.clone()));

        let grid = gtk::Grid::builder()
            .column_spacing(6)
            .row_spacing(6)
            .hexpand(true)
            .build();
        for (row, tag) in tags.iter().enumerate() {
            let label = gtk::Label::new(Some(&tag_label(&tag.key)));
            label.set_xalign(0.0);
            label.set_tooltip_text(Some(&format!("Output tag: {}", tag.key)));
            label.add_css_class("info-label");
            let value = gtk::Entry::builder().text(&tag.value).hexpand(true).build();
            let key = tag.key.clone();
            let callback = on_action.clone();
            value.connect_changed(move |value| {
                callback(InfoAction::SetTag {
                    title_id,
                    key: key.clone(),
                    value: value.text().to_string(),
                    all_tracks: false,
                });
            });
            let save = gtk::Button::with_label("Apply to all album tracks");
            save.set_tooltip_text(Some(
                "Replace this tag on every track with the current value",
            ));
            let key = tag.key.clone();
            let callback = on_action.clone();
            let edited_value = value.clone();
            save.connect_clicked(move |_| {
                callback(InfoAction::SetTag {
                    title_id,
                    key: key.clone(),
                    value: edited_value.text().to_string(),
                    all_tracks: true,
                });
            });
            grid.attach(&label, 0, row as i32, 1, 1);
            grid.attach(&value, 1, row as i32, 1, 1);
            grid.attach(&save, 2, row as i32, 1, 1);
        }
        self.tags_content.append(&grid);
        self.tags_content
            .append(&add_tag_row(title_id, tag_keys, on_action));
    }

    fn render_tags_unavailable(&self, message: &str) {
        clear(&self.tags_content);
        let label = gtk::Label::new(Some(message));
        label.set_xalign(0.0);
        label.set_yalign(0.0);
        label.set_wrap(true);
        label.add_css_class("row-detail");
        self.tags_content.append(&label);
    }

    fn render_metadata(
        &self,
        catalog: &MediaCatalog,
        selection: &Selection,
        episode_metadata: Option<&EpisodeMetadataContext>,
        can_undo_metadata: bool,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        clear(&self.metadata_content);
        let title_id = match selection {
            Selection::Title(title_id) | Selection::Track { title_id, .. } => Some(*title_id),
            Selection::Source => None,
        };
        let Some(title_id) = title_id else {
            let fields = if let Some(suggestion) = catalog.video_metadata_suggestions.first() {
                vec![
                    InfoField::new(
                        "Pending proposals",
                        catalog.video_metadata_suggestions.len().to_string(),
                    ),
                    InfoField::new("Shared evidence", bullet_list(&suggestion.evidence)),
                    InfoField::new("Shared assumptions", bullet_list(&suggestion.assumptions)),
                ]
            } else {
                vec![InfoField::new(
                    "Metadata",
                    "Select a video title to inspect its metadata.",
                )]
            };
            self.metadata_content.append(&fields_grid(&fields));
            return;
        };
        let Some(title) = catalog.titles.iter().find(|title| title.id == title_id) else {
            self.render_metadata_unavailable("The selected video title is no longer available.");
            return;
        };
        if let Some(suggestion) = catalog
            .video_metadata_suggestions
            .iter()
            .find(|suggestion| suggestion.title_id == title.id)
        {
            let mut fields = vec![
                InfoField::new("Status", "Pending — not applied"),
                InfoField::new("Current name", &title.name),
                InfoField::new("Proposed name", &suggestion.name),
                InfoField::new(
                    "Match score",
                    format!("{}/100", suggestion.metadata.attribution.confidence_percent),
                ),
                InfoField::new("Method", &suggestion.metadata.attribution.match_method),
            ];
            append_video_metadata_fields(&mut fields, &suggestion.metadata);
            if !suggestion.evidence.is_empty() {
                fields.push(InfoField::new(
                    "Why proposed",
                    bullet_list(&suggestion.evidence),
                ));
            }
            if !suggestion.assumptions.is_empty() {
                fields.push(InfoField::new(
                    "Assumptions",
                    bullet_list(&suggestion.assumptions),
                ));
            }
            self.metadata_content.append(&fields_grid(&fields));
            let guidance = gtk::Label::new(Some(
                "Use Review metadata above the content list to compare and apply pending changes.",
            ));
            guidance.set_xalign(0.0);
            guidance.set_wrap(true);
            guidance.add_css_class("row-detail");
            self.metadata_content.append(&guidance);
        } else if let Some(metadata) = &title.video_metadata {
            let mut fields = vec![
                InfoField::new("Status", "Applied"),
                InfoField::new("Name", &title.name),
                InfoField::new("Metadata source", &metadata.attribution.provider),
                InfoField::new(
                    "Match score",
                    format!("{}/100", metadata.attribution.confidence_percent),
                ),
                InfoField::new("Method", &metadata.attribution.match_method),
            ];
            append_video_metadata_fields(&mut fields, metadata);
            self.metadata_content.append(&fields_grid(&fields));
            if let Some(context) = episode_metadata {
                let heading = gtk::Label::new(Some("Change the applied episode"));
                heading.set_xalign(0.0);
                heading.add_css_class("config-label");
                self.metadata_content.append(&heading);
                self.append_episode_assignment(catalog, title_id, context, on_action.clone());
            }
            let remove = gtk::Button::with_label(if can_undo_metadata {
                "Undo metadata and naming changes"
            } else {
                "Remove metadata"
            });
            remove.set_halign(gtk::Align::Start);
            remove.set_tooltip_text(Some(
                "Restore the previous name, filename, tags, track names and metadata",
            ));
            remove.connect_clicked(move |_| {
                on_action(InfoAction::RemoveVideoMetadata { title_id });
            });
            self.metadata_content.append(&remove);
        } else if let Some(context) = episode_metadata {
            let fields = [InfoField::new(
                "Status",
                "No automatic proposal — choose an episode below to assign the matched season metadata manually.",
            )];
            self.metadata_content.append(&fields_grid(&fields));
            self.append_episode_assignment(catalog, title_id, context, on_action);
        } else {
            self.render_metadata_unavailable(
                "No exact metadata or probable episode proposal is available for this title.",
            );
        }
    }

    fn append_episode_assignment(
        &self,
        catalog: &MediaCatalog,
        title_id: u32,
        context: &EpisodeMetadataContext,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        if context.choices.is_empty() {
            return;
        }
        let season_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        season_row.add_css_class("episode-assignment-row");
        season_row.append(&gtk::Label::new(Some("Season")));
        let season = gtk::Entry::builder()
            .text(context.metadata.season.unwrap_or_default().to_string())
            .width_chars(4)
            .max_width_chars(4)
            .input_purpose(gtk::InputPurpose::Number)
            .build();
        let load = gtk::Button::with_label("Load season");
        let selected_season = season.clone();
        let callback = on_action.clone();
        load.connect_clicked(move |_| {
            if let Ok(season) = selected_season.text().trim().parse::<u32>()
                && season > 0
            {
                callback(InfoAction::SetEpisodeSeason(season));
            }
        });
        season_row.append(&season);
        season_row.append(&load);
        self.metadata_content.append(&season_row);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.add_css_class("episode-assignment-row");
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
        chooser.set_hexpand(true);
        chooser.set_selected(default_episode_choice(catalog, context) as u32);
        chooser.set_tooltip_text(Some(
            "Choose an episode from the matched season for this existing disc title",
        ));
        let apply = gtk::Button::with_label(
            if catalog
                .titles
                .iter()
                .find(|title| title.id == title_id)
                .is_some_and(|title| title.video_metadata.is_some())
            {
                "Change episode"
            } else {
                "Assign episode"
            },
        );
        apply.add_css_class("suggested-action");
        let selected = chooser.clone();
        apply.connect_clicked(move |_| {
            if let Some(episode) = episodes.get(selected.selected() as usize) {
                on_action(InfoAction::AssignEpisode {
                    title_id,
                    episode: *episode,
                });
            }
        });
        row.append(&chooser);
        row.append(&apply);
        self.metadata_content.append(&row);
    }

    fn render_metadata_unavailable(&self, message: &str) {
        clear(&self.metadata_content);
        let label = gtk::Label::new(Some(message));
        label.set_xalign(0.0);
        label.set_yalign(0.0);
        label.set_wrap(true);
        label.add_css_class("row-detail");
        self.metadata_content.append(&label);
    }

    fn append_track_artwork(
        &self,
        cover_art: Option<&CoverArt>,
        title_id: u32,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        const ARTWORK_SIZE: i32 = 128;
        let artwork = artwork_control(cover_art, Some(title_id), ARTWORK_SIZE, on_action);
        artwork.set_tooltip_text(Some("Track cover artwork"));
        artwork.set_halign(gtk::Align::End);
        artwork.set_valign(gtk::Align::Start);
        artwork.set_margin_top(9);
        artwork.set_margin_end(9);
        clear(&self.info_artwork);
        self.info_artwork.append(&artwork);
    }

    fn remove_info_artwork(&self) {
        clear(&self.info_artwork);
    }

    fn append_readonly_artwork(&self, cover_art: Option<&CoverArt>, tooltip: &str) {
        let Some(cover_art) = cover_art else {
            return;
        };
        let Some(pixbuf) = gdk_pixbuf::Pixbuf::from_read(Cursor::new(cover_art.data.clone())).ok()
        else {
            return;
        };
        let (width, height) = artwork_size(pixbuf.width(), pixbuf.height(), 128);
        let picture = gtk::Picture::for_pixbuf(&pixbuf);
        picture.set_keep_aspect_ratio(true);
        picture.set_can_shrink(true);
        picture.set_size_request(width, height);
        picture.set_tooltip_text(Some(tooltip));
        picture.set_margin_top(9);
        picture.set_margin_end(9);
        clear(&self.info_artwork);
        self.info_artwork.append(&picture);
    }

    fn append_output_file_editor(
        &self,
        row: usize,
        title_id: u32,
        output_file: &str,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        let label = gtk::Label::new(Some("File name"));
        label.set_xalign(0.0);
        label.add_css_class("info-label");
        let entry = gtk::Entry::builder()
            .text(output_file)
            .hexpand(true)
            .width_chars(32)
            .tooltip_text(output_file)
            .build();
        entry.connect_changed(move |entry| {
            on_action(InfoAction::SetOutputFile {
                title_id,
                output_file: entry.text().to_string(),
            });
        });
        self.info_grid.attach(&label, 0, row as i32, 1, 1);
        self.info_grid.attach(&entry, 1, row as i32, 1, 1);
    }

    fn append_title_name_editor(
        &self,
        row: usize,
        title_id: u32,
        name: &str,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        let label = gtk::Label::new(Some("Name"));
        label.set_xalign(0.0);
        label.add_css_class("info-label");
        let entry = wide_entry(name);
        entry.connect_changed(move |entry| {
            on_action(InfoAction::SetTitleName {
                title_id,
                name: entry.text().to_string(),
            });
        });
        self.info_grid.attach(&label, 0, row as i32, 1, 1);
        self.info_grid.attach(&entry, 1, row as i32, 1, 1);
    }

    fn append_track_name_editor(
        &self,
        row: usize,
        title_id: u32,
        stream_index: u32,
        name: &str,
        on_action: Rc<dyn Fn(InfoAction)>,
    ) {
        let label = gtk::Label::new(Some("Name"));
        label.set_xalign(0.0);
        label.add_css_class("info-label");
        let entry = wide_entry(name);
        entry.set_placeholder_text(Some("Unnamed track"));
        entry.connect_changed(move |entry| {
            on_action(InfoAction::SetTrackName {
                title_id,
                stream_index,
                name: entry.text().to_string(),
            });
        });
        self.info_grid.attach(&label, 0, row as i32, 1, 1);
        self.info_grid.attach(&entry, 1, row as i32, 1, 1);
    }
}

fn wide_entry(value: &str) -> gtk::Entry {
    gtk::Entry::builder()
        .text(value)
        .hexpand(true)
        .width_chars(32)
        .tooltip_text(value)
        .build()
}

fn default_episode_choice(catalog: &MediaCatalog, context: &EpisodeMetadataContext) -> usize {
    let used = catalog
        .titles
        .iter()
        .filter_map(|title| title.video_metadata.as_ref())
        .filter(|metadata| metadata.season == context.metadata.season)
        .filter_map(|metadata| metadata.episode)
        .chain(
            catalog
                .video_metadata_suggestions
                .iter()
                .filter_map(|suggestion| suggestion.metadata.episode),
        )
        .collect::<BTreeSet<_>>();
    let missing_inside_range =
        used.iter()
            .min()
            .zip(used.iter().max())
            .and_then(|(first, last)| {
                context.choices.iter().position(|choice| {
                    choice.episode >= *first
                        && choice.episode <= *last
                        && !used.contains(&choice.episode)
                })
            });
    let after_last = used.iter().max().and_then(|last| {
        context
            .choices
            .iter()
            .position(|choice| choice.episode > *last && !used.contains(&choice.episode))
    });
    missing_inside_range
        .or(after_last)
        .or_else(|| {
            context
                .choices
                .iter()
                .position(|choice| !used.contains(&choice.episode))
        })
        .unwrap_or_default()
}

fn append_video_metadata_fields(fields: &mut Vec<InfoField>, metadata: &VideoMetadata) {
    if let Some(content_type) = &metadata.content_type {
        fields.push(InfoField::new("Content role", content_type));
    }
    if let Some(year) = metadata.year {
        fields.push(InfoField::new("Year", year.to_string()));
    }
    if let Some(season) = metadata.season {
        fields.push(InfoField::new("Season", season.to_string()));
    }
    if let Some(episode) = metadata.episode {
        fields.push(InfoField::new("Episode", episode.to_string()));
    }
    if let Some(tmdb_id) = metadata.tmdb_id {
        fields.push(InfoField::new("TMDB ID", tmdb_id.to_string()));
    }
    if let Some(tvdb_id) = metadata.tvdb_id {
        fields.push(InfoField::new("TVDB ID", tvdb_id.to_string()));
    }
}

fn bullet_list(items: &[String]) -> String {
    if items.is_empty() {
        "Not recorded".to_owned()
    } else {
        format!("• {}", items.join("\n• "))
    }
}

fn fields_grid(fields: &[InfoField]) -> gtk::Grid {
    let grid = gtk::Grid::builder()
        .column_spacing(14)
        .row_spacing(6)
        .valign(gtk::Align::Start)
        .hexpand(true)
        .build();
    grid.add_css_class("info-grid");
    for (row, field) in fields.iter().enumerate() {
        let label = gtk::Label::new(Some(&field.label));
        label.set_xalign(0.0);
        label.set_yalign(0.0);
        label.add_css_class("info-label");
        let value = gtk::Label::new(Some(&field.value));
        value.set_xalign(0.0);
        value.set_yalign(0.0);
        value.set_wrap(true);
        value.set_selectable(true);
        value.set_hexpand(true);
        value.add_css_class("info-value");
        grid.attach(&label, 0, row as i32, 1, 1);
        grid.attach(&value, 1, row as i32, 1, 1);
    }
    grid
}

fn cover_art_editor(cover_art: Option<&CoverArt>, on_action: Rc<dyn Fn(InfoAction)>) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let label = gtk::Label::new(Some("Album artwork"));
    label.set_xalign(0.0);
    label.add_css_class("info-label");
    section.append(&label);
    section.append(&artwork_control(cover_art, None, 128, on_action));
    section
}

fn artwork_control(
    cover_art: Option<&CoverArt>,
    title_id: Option<u32>,
    size: i32,
    on_action: Rc<dyn Fn(InfoAction)>,
) -> gtk::Fixed {
    let pixbuf = cover_art.and_then(|cover_art| {
        gdk_pixbuf::Pixbuf::from_read(Cursor::new(cover_art.data.clone())).ok()
    });
    let (width, height) = pixbuf
        .as_ref()
        .map(|pixbuf| artwork_size(pixbuf.width(), pixbuf.height(), size))
        .unwrap_or((size, size));

    let overlay = gtk::Overlay::new();
    overlay.set_size_request(width, height);
    overlay.set_halign(gtk::Align::Start);
    overlay.set_valign(gtk::Align::Start);
    overlay.set_hexpand(false);
    overlay.set_vexpand(false);
    overlay.add_css_class("artwork-control");

    if let Some(pixbuf) = pixbuf {
        let picture = gtk::Picture::for_pixbuf(&pixbuf);
        picture.set_keep_aspect_ratio(true);
        picture.set_can_shrink(true);
        picture.set_size_request(width, height);
        picture.set_hexpand(false);
        picture.set_vexpand(false);
        picture.add_css_class("cover-art");
        overlay.set_child(Some(&picture));
    } else {
        let placeholder = gtk::Image::from_icon_name("image-x-generic-symbolic");
        placeholder.set_pixel_size(size / 2);
        placeholder.set_size_request(size, size);
        placeholder.add_css_class("cover-art-placeholder");
        overlay.set_child(Some(&placeholder));
    }

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    actions.set_halign(gtk::Align::End);
    actions.set_valign(gtk::Align::End);
    actions.set_margin_end(4);
    actions.set_margin_bottom(4);
    actions.set_opacity(0.0);
    actions.set_can_target(false);
    actions.add_css_class("artwork-overlay-actions");

    let remove = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Remove artwork")
        .has_frame(false)
        .sensitive(cover_art.is_some())
        .build();
    let callback = on_action.clone();
    remove.connect_clicked(move |_| callback(InfoAction::RemoveCoverArt { title_id }));
    actions.append(&remove);
    let choose = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text("Choose artwork")
        .has_frame(false)
        .build();
    choose.connect_clicked(move |_| on_action(InfoAction::ChooseCoverArt { title_id }));
    actions.append(&choose);
    overlay.add_overlay(&actions);

    let hover = gtk::EventControllerMotion::new();
    hover.connect_enter({
        let actions = actions.clone();
        move |_, _, _| {
            actions.set_opacity(1.0);
            actions.set_can_target(true);
        }
    });
    hover.connect_leave(move |_| {
        actions.set_opacity(0.0);
        actions.set_can_target(false);
    });
    overlay.add_controller(hover);

    // GtkBox allocates its children across the full cross-axis. Keeping the styled
    // overlay inside a Fixed prevents that allocation from becoming visible around
    // the image while preserving the image's decoded aspect ratio.
    let host = gtk::Fixed::new();
    host.set_size_request(width, height);
    host.set_halign(gtk::Align::Start);
    host.set_valign(gtk::Align::Start);
    host.set_hexpand(false);
    host.set_vexpand(false);
    host.put(&overlay, 0.0, 0.0);
    host
}

fn artwork_size(width: i32, height: i32, maximum: i32) -> (i32, i32) {
    if width <= 0 || height <= 0 {
        return (maximum, maximum);
    }
    if width >= height {
        (
            maximum,
            ((i64::from(maximum) * i64::from(height)) / i64::from(width)).max(1) as i32,
        )
    } else {
        (
            ((i64::from(maximum) * i64::from(width)) / i64::from(height)).max(1) as i32,
            maximum,
        )
    }
}

fn add_tag_row(title_id: u32, tag_keys: &[String], on_action: Rc<dyn Fn(InfoAction)>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("add-tag-row");
    let label = gtk::Label::new(Some("Add a tag"));
    label.set_xalign(0.0);
    label.add_css_class("info-label");
    row.append(&label);

    let inputs = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let key = gtk::ComboBoxText::with_entry();
    key.set_hexpand(true);
    for suggestion in tag_keys {
        key.append_text(suggestion);
    }
    let key_entry = key
        .child()
        .and_then(|child| child.downcast::<gtk::Entry>().ok())
        .expect("an editable tag combo box must contain an entry");
    key_entry.set_placeholder_text(Some("Tag name"));
    key_entry.set_tooltip_text(Some("Type a tag name or choose a suggested key"));
    let value = gtk::Entry::builder()
        .placeholder_text("Value")
        .hexpand(true)
        .build();
    inputs.append(&key);
    inputs.append(&value);
    row.append(&inputs);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_halign(gtk::Align::End);
    let add = gtk::Button::with_label("Add to this track");
    add.set_tooltip_text(Some("Add this tag to the selected track"));
    let callback = on_action.clone();
    let selected_key = key_entry.clone();
    let selected_value = value.clone();
    add.connect_clicked(move |_| {
        callback(InfoAction::AddTag {
            title_id,
            key: tag_key_from_input(&selected_key.text()),
            value: selected_value.text().to_string(),
            all_tracks: false,
        });
    });
    actions.append(&add);
    let add_all = gtk::Button::with_label("Add to all album tracks");
    add_all.set_tooltip_text(Some("Add or replace this tag on every track"));
    add_all.connect_clicked(move |_| {
        on_action(InfoAction::AddTag {
            title_id,
            key: tag_key_from_input(&key_entry.text()),
            value: value.text().to_string(),
            all_tracks: true,
        });
    });
    actions.append(&add_all);
    row.append(&actions);
    row
}

fn tag_key_suggestions(catalog: &MediaCatalog) -> Vec<String> {
    let mut keys = [
        "album",
        "album_artist",
        "albumartistsort",
        "artist",
        "artists",
        "artistsort",
        "barcode",
        "catalognumber",
        "comment",
        "composer",
        "copyright",
        "date",
        "discnumber",
        "genre",
        "isrc",
        "label",
        "lyrics",
        "media",
        "musicbrainz_albumid",
        "musicbrainz_albumartistid",
        "musicbrainz_artistid",
        "musicbrainz_discid",
        "musicbrainz_recordingid",
        "musicbrainz_releasegroupid",
        "musicbrainz_releasetrackid",
        "musicbrainz_trackid",
        "originaldate",
        "originalyear",
        "performer",
        "releasecountry",
        "releasestatus",
        "releasetype",
        "script",
        "title",
        "track",
        "totaldiscs",
        "totaltracks",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    for tag in catalog.titles.iter().flat_map(|title| &title.tags) {
        keys.insert(tag.key.trim().to_ascii_lowercase());
    }
    let mut labels = keys
        .into_iter()
        .filter(|key| !key.is_empty())
        .map(|key| tag_label(&key))
        .collect::<Vec<_>>();
    labels.sort_by_key(|label| label.to_ascii_lowercase());
    labels.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    labels
}

fn clear(container: &impl IsA<gtk::Widget>) {
    while let Some(child) = container.first_child() {
        if let Some(grid) = container.dynamic_cast_ref::<gtk::Grid>() {
            grid.remove(&child);
        } else if let Some(box_) = container.dynamic_cast_ref::<gtk::Box>() {
            box_.remove(&child);
        }
    }
}

fn tag_label(key: &str) -> String {
    if let Some((_, label)) = PICARD_TAG_LABELS
        .iter()
        .find(|(tag, _)| tag.eq_ignore_ascii_case(key))
    {
        return (*label).to_owned();
    }
    key.replace('_', " ")
        .split_whitespace()
        .map(title_case_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn tag_key_from_input(input: &str) -> String {
    let input = input.trim();
    if let Some((key, _)) = PICARD_TAG_LABELS
        .iter()
        .find(|(key, label)| key.eq_ignore_ascii_case(input) || label.eq_ignore_ascii_case(input))
    {
        return (*key).to_owned();
    }
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
        .to_ascii_lowercase()
}

fn title_case_word(word: &str) -> String {
    let mut characters = word.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_uppercase().chain(characters).collect()
}

const PICARD_TAG_LABELS: &[(&str, &str)] = &[
    ("album", "Album"),
    ("album_artist", "Album Artist"),
    ("albumartistsort", "Album Artist Sort Order"),
    ("artist", "Artist"),
    ("artists", "Artists"),
    ("artistsort", "Artist Sort Order"),
    ("barcode", "Barcode"),
    ("catalognumber", "Catalog Number"),
    ("comment", "Comment"),
    ("composer", "Composer"),
    ("copyright", "Copyright"),
    ("date", "Date"),
    ("discnumber", "Disc Number"),
    ("genre", "Genre"),
    ("isrc", "ISRC"),
    ("label", "Record Label"),
    ("lyrics", "Lyrics"),
    ("media", "Media"),
    ("musicbrainz_albumartistid", "MusicBrainz Album Artist ID"),
    ("musicbrainz_albumid", "MusicBrainz Release ID"),
    ("musicbrainz_artistid", "MusicBrainz Artist ID"),
    ("musicbrainz_discid", "MusicBrainz Disc ID"),
    ("musicbrainz_recordingid", "MusicBrainz Recording ID"),
    ("musicbrainz_releasegroupid", "MusicBrainz Release Group ID"),
    ("musicbrainz_releasetrackid", "MusicBrainz Release Track ID"),
    ("musicbrainz_trackid", "MusicBrainz Track ID"),
    ("originaldate", "Original Release Date"),
    ("originalyear", "Original Release Year"),
    ("performer", "Performer"),
    ("releasecountry", "Release Country"),
    ("releasestatus", "Release Status"),
    ("releasetype", "Release Type"),
    ("script", "Script"),
    ("title", "Title"),
    ("totaldiscs", "Total Discs"),
    ("totaltracks", "Total Tracks"),
    ("track", "Track Number"),
];

fn format_playlist_ranges(key: &str) -> Option<String> {
    key.split_terminator(';')
        .map(|item| {
            let mut fields = item.split(':');
            let clip_id = fields.next()?;
            let in_time = u64::from_str_radix(fields.next()?, 16).ok()?;
            let out_time = u64::from_str_radix(fields.next()?, 16).ok()?;
            fields.next().is_none().then(|| {
                format!(
                    "{clip_id}.m2ts {}–{}",
                    format_duration(in_time / 90),
                    format_duration(out_time / 90)
                )
            })
        })
        .collect::<Option<Vec<_>>>()
        .filter(|items| !items.is_empty())
        .map(|items| items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::{artwork_size, format_playlist_ranges, tag_key_from_input, tag_label};

    #[test]
    fn artwork_bounds_follow_the_source_aspect_ratio() {
        assert_eq!(artwork_size(500, 400, 128), (128, 102));
        assert_eq!(artwork_size(400, 500, 128), (102, 128));
        assert_eq!(artwork_size(500, 500, 128), (128, 128));
    }

    #[test]
    fn formats_exact_playlist_ranges_for_diagnostics() {
        assert_eq!(
            format_playlist_ranges(
                "00001:0000000000000000:00000000134fd900;\
                 00042:0000000000015f90:000000000002bf20;"
            )
            .as_deref(),
            Some("00001.m2ts 0:00:00–1:00:00, 00042.m2ts 0:00:01–0:00:02")
        );
    }

    #[test]
    fn tag_names_use_picard_style_labels_and_stable_keys() {
        assert_eq!(tag_label("album_artist"), "Album Artist");
        assert_eq!(tag_label("musicbrainz_albumid"), "MusicBrainz Release ID");
        assert_eq!(tag_label("custom_tag"), "Custom Tag");
        assert_eq!(tag_key_from_input("Album Artist"), "album_artist");
        assert_eq!(tag_key_from_input("Track Number"), "track");
        assert_eq!(tag_key_from_input("Custom Tag"), "custom_tag");
    }
}
