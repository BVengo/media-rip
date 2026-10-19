use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap},
    rc::Rc,
};

use gtk::{glib, prelude::*};
use media_rip_core::{
    DiscKind, MediaCatalog, Title,
    util::{format_bytes, format_duration},
};

use crate::app::Selection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentAction {
    SetAllChecked(bool),
    SelectTitle(u32),
    SelectTrack {
        title_id: u32,
        stream_index: u32,
    },
    ToggleExpanded(u32),
    ApplySuggestions(Vec<u32>),
    ApplyAudioCandidate(String),
    SkipMetadata,
    SetSuggestionSeason(u32),
    SetSuggestionFirstEpisode(u32),
    SetTitleChecked {
        title_id: u32,
        selected: bool,
    },
    SetTrackChecked {
        title_id: u32,
        stream_index: u32,
        selected: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ContentTree {
    pub stack: gtk::Stack,
    title_box: gtk::Box,
    scroll: gtk::ScrolledWindow,
    pending_scroll_restore: Rc<RefCell<Option<glib::SignalHandlerId>>>,
    expanded_rows: Rc<RefCell<HashMap<u32, ExpandedRow>>>,
    select_all: Rc<RefCell<Option<gtk::CheckButton>>>,
    select_all_state: Rc<Cell<CheckState>>,
    updating_checks: Rc<Cell<bool>>,
    pub idle_open: gtk::Button,
    idle_image: gtk::Image,
    idle_spinner: gtk::Spinner,
    idle_label: gtk::Label,
    idle_hint: gtk::Label,
}

#[derive(Debug, Clone)]
struct ExpandedRow {
    row: gtk::Box,
    button: gtk::Button,
    selected: gtk::CheckButton,
    check_state: Rc<Cell<CheckState>>,
    name: gtk::Label,
    tracks: gtk::Box,
    track_buttons: HashMap<u32, gtk::CheckButton>,
    track_rows: HashMap<u32, gtk::Box>,
    track_descriptions: HashMap<u32, gtk::Label>,
}

impl ContentTree {
    pub fn new() -> Self {
        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);

        let idle = gtk::Box::new(gtk::Orientation::Vertical, 10);
        idle.set_halign(gtk::Align::Center);
        idle.set_valign(gtk::Align::Center);
        let idle_open = gtk::Button::new();
        idle_open.add_css_class("idle-disc-button");
        let button_content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let idle_image = gtk::Image::from_icon_name("media-optical-symbolic");
        idle_image.set_pixel_size(112);
        let idle_spinner = gtk::Spinner::new();
        idle_spinner.set_size_request(112, 112);
        idle_spinner.set_visible(false);
        let idle_label = gtk::Label::new(Some("Open disc"));
        idle_label.add_css_class("idle-title");
        button_content.append(&idle_image);
        button_content.append(&idle_spinner);
        button_content.append(&idle_label);
        idle_open.set_child(Some(&button_content));
        let idle_hint = gtk::Label::new(Some("No disc detected. Insert an optical disc to begin."));
        idle_hint.add_css_class("idle-caption");
        idle.append(&idle_open);
        idle.append(&idle_hint);
        stack.add_named(&idle, Some("empty"));

        let title_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        title_box.add_css_class("content-surface");
        title_box.set_margin_end(8);
        let scroll = gtk::ScrolledWindow::builder()
            .overlay_scrolling(false)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&title_box)
            .build();
        stack.add_named(&scroll, Some("titles"));
        stack.set_visible_child_name("empty");

        Self {
            stack,
            title_box,
            scroll,
            pending_scroll_restore: Rc::new(RefCell::new(None)),
            expanded_rows: Rc::new(RefCell::new(HashMap::new())),
            select_all: Rc::new(RefCell::new(None)),
            select_all_state: Rc::new(Cell::new(CheckState::None)),
            updating_checks: Rc::new(Cell::new(false)),
            idle_open,
            idle_image,
            idle_spinner,
            idle_label,
            idle_hint,
        }
    }

    pub fn set_read_only(&self, read_only: bool) {
        if let Some(check) = self.select_all.borrow().as_ref() {
            check.set_sensitive(!read_only);
        }
        for row in self.expanded_rows.borrow().values() {
            row.selected.set_sensitive(!read_only);
            for check in row.track_buttons.values() {
                check.set_sensitive(!read_only);
            }
        }
    }

    pub fn set_disc_state(&self, state: DiscUiState, enabled: bool) {
        self.idle_open.set_sensitive(enabled);
        match state {
            DiscUiState::Absent => {
                self.idle_spinner.stop();
                self.idle_spinner.set_visible(false);
                self.idle_image.set_visible(true);
                self.idle_image
                    .set_icon_name(Some("media-optical-symbolic"));
                self.idle_label.set_text("Open disc");
                self.idle_hint
                    .set_text("No disc detected. Insert an optical disc to begin.");
            }
            DiscUiState::Reading => {
                self.idle_image.set_visible(false);
                self.idle_spinner.set_visible(true);
                self.idle_spinner.start();
                self.idle_label.set_text("Reading disc…");
                self.idle_hint
                    .set_text("A disc was detected. Waiting for the system to identify it.");
            }
            DiscUiState::Ready { kind, label } => {
                self.idle_spinner.stop();
                self.idle_spinner.set_visible(false);
                self.idle_image.set_visible(true);
                self.idle_image.set_icon_name(Some(kind.icon_name()));
                self.idle_label
                    .set_text(&format!("Open {}", kind.display_name()));
                self.idle_hint.set_text(
                    label
                        .as_deref()
                        .unwrap_or("Disc ready. Open it to read the available content."),
                );
            }
        }
    }

    pub fn clear(&self) {
        if let Some(handler) = self.pending_scroll_restore.borrow_mut().take() {
            self.scroll.vadjustment().disconnect(handler);
        }
        self.expanded_rows.borrow_mut().clear();
        self.select_all.borrow_mut().take();
        clear_box(&self.title_box);
        self.stack.set_visible_child_name("empty");
    }

    pub fn render(
        &self,
        catalog: &MediaCatalog,
        expanded: &BTreeSet<u32>,
        selection: &Selection,
        on_action: Rc<dyn Fn(ContentAction)>,
    ) {
        let adjustment = self.scroll.vadjustment();
        let scroll_position = adjustment.value();
        self.expanded_rows.borrow_mut().clear();
        self.select_all.borrow_mut().take();
        clear_box(&self.title_box);
        let select_all = select_all_checkbox(
            catalog,
            on_action.clone(),
            self.select_all_state.clone(),
            self.updating_checks.clone(),
        );
        *self.select_all.borrow_mut() = Some(select_all.clone());
        let columns = TitleColumns::new();
        self.title_box.append(&columns.header(&select_all));
        let duplicate_originals = duplicate_originals(&catalog.titles);
        let review_titles = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let mut review_count = 0;
        for title in &catalog.titles {
            let section = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let (header, button, selected, check_state, name) = title_row(
                title,
                catalog,
                &columns,
                duplicate_originals.get(&title.id).map(String::as_str),
                expanded.contains(&title.id),
                on_action.clone(),
                self.updating_checks.clone(),
            );
            section.append(&header);
            let tracks = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let mut track_buttons = HashMap::new();
            let mut track_rows = HashMap::new();
            let mut track_descriptions = HashMap::new();
            for track in &title.tracks {
                let (row, selected, description) = track_row(
                    title.id,
                    track,
                    on_action.clone(),
                    self.updating_checks.clone(),
                );
                tracks.append(&row);
                track_buttons.insert(track.index, selected);
                track_rows.insert(track.index, row);
                track_descriptions.insert(track.index, description);
            }
            tracks.set_visible(expanded.contains(&title.id));
            section.append(&tracks);
            let needs_review = catalog.source.kind == DiscKind::Dvd
                && !title.selected
                && (duplicate_originals.contains_key(&title.id)
                    || title
                        .dvd
                        .as_ref()
                        .is_some_and(|dvd| !dvd.warnings.is_empty()));
            if needs_review {
                review_titles.append(&section);
                review_count += 1;
            } else {
                self.title_box.append(&section);
            }
            self.expanded_rows.borrow_mut().insert(
                title.id,
                ExpandedRow {
                    row: header,
                    button,
                    selected,
                    check_state,
                    name,
                    tracks,
                    track_buttons,
                    track_rows,
                    track_descriptions,
                },
            );
        }
        if review_count > 0 {
            let review = gtk::Expander::new(Some(&format!(
                "{review_count} duplicate or unverified DVD title(s)"
            )));
            review.set_tooltip_text(Some("DVD titles are playback paths. Some reference the same video sectors; others have structural problems. These titles start unchecked."));
            review.set_child(Some(&review_titles));
            self.title_box.append(&review);
        }
        self.sync_view_selection(selection);
        self.stack.set_visible_child_name("titles");
        if let Some(handler) = self.pending_scroll_restore.borrow_mut().take() {
            adjustment.disconnect(handler);
        }
        let pending = self.pending_scroll_restore.clone();
        let handler = adjustment.connect_changed(move |adjustment| {
            let maximum = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
            adjustment.set_value(scroll_position.clamp(adjustment.lower(), maximum));
            if let Some(handler) = pending.borrow_mut().take() {
                adjustment.disconnect(handler);
            }
        });
        *self.pending_scroll_restore.borrow_mut() = Some(handler);
        self.scroll.queue_resize();
    }

    pub fn set_expanded(&self, title_id: u32, expanded: bool) {
        let rows = self.expanded_rows.borrow();
        let Some(row) = rows.get(&title_id) else {
            return;
        };
        row.button.set_icon_name(if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        });
        row.tracks.set_visible(expanded);
    }

    pub fn sync_title_checks(&self, title: &Title) {
        let rows = self.expanded_rows.borrow();
        let Some(row) = rows.get(&title.id) else {
            return;
        };
        self.updating_checks.set(true);
        let check_state = title_check_state(title);
        row.check_state.set(check_state);
        row.selected.set_active(check_state == CheckState::All);
        row.selected
            .set_inconsistent(check_state == CheckState::Some);
        row.selected.set_tooltip_text(Some(check_state.tooltip()));
        for track in &title.tracks {
            if let Some(button) = row.track_buttons.get(&track.index) {
                button.set_active(track.selected);
            }
        }
        self.updating_checks.set(false);
    }

    pub fn sync_select_all(&self, catalog: &MediaCatalog) {
        let Some(button) = self.select_all.borrow().as_ref().cloned() else {
            return;
        };
        self.updating_checks.set(true);
        let check_state = select_all_check_state(catalog);
        self.select_all_state.set(check_state);
        set_check_button_state(&button, check_state);
        self.updating_checks.set(false);
    }

    pub fn sync_title_name(&self, title_id: u32, name: &str) {
        if let Some(row) = self.expanded_rows.borrow().get(&title_id) {
            row.name.set_text(name);
            row.name.set_tooltip_text(Some(name));
        }
    }

    pub fn sync_track_description(
        &self,
        title_id: u32,
        stream_index: u32,
        track: &media_rip_core::Track,
    ) {
        if let Some(description) = self
            .expanded_rows
            .borrow()
            .get(&title_id)
            .and_then(|row| row.track_descriptions.get(&stream_index))
        {
            description.set_text(&track_description(track));
        }
    }

    pub fn sync_all_checks(&self, catalog: &MediaCatalog) {
        for title in &catalog.titles {
            self.sync_title_checks(title);
        }
        self.sync_select_all(catalog);
    }

    pub fn sync_view_selection(&self, selection: &Selection) {
        for (title_id, expanded_row) in self.expanded_rows.borrow().iter() {
            set_viewing(
                &expanded_row.row,
                matches!(selection, Selection::Title(selected_id) if selected_id == title_id),
            );
            for (stream_index, track_row) in &expanded_row.track_rows {
                set_viewing(
                    track_row,
                    matches!(
                        selection,
                        Selection::Track {
                            title_id: selected_title_id,
                            stream_index: selected_stream_index,
                        } if selected_title_id == title_id && selected_stream_index == stream_index
                    ),
                );
            }
        }
    }
}

fn set_viewing(row: &gtk::Box, viewing: bool) {
    if viewing {
        row.add_css_class("viewing-row");
    } else {
        row.remove_css_class("viewing-row");
    }
}

#[derive(Debug, Clone)]
pub(crate) enum DiscUiState {
    Absent,
    Reading,
    Ready {
        kind: DiscKind,
        label: Option<String>,
    },
}

/// Shared column widths keep headers and rows aligned as the window resizes.
struct TitleColumns {
    controls: gtk::SizeGroup,
    kind: gtk::SizeGroup,
    duration: gtk::SizeGroup,
    size: gtk::SizeGroup,
    chapters: gtk::SizeGroup,
}

impl TitleColumns {
    fn new() -> Self {
        Self {
            controls: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            kind: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            duration: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            size: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            chapters: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
        }
    }

    fn cell(&self, value: &str, group: &gtk::SizeGroup, numeric: bool) -> gtk::Label {
        let label = gtk::Label::new(Some(value));
        label.set_xalign(if numeric { 1.0 } else { 0.0 });
        label.set_margin_start(12);
        label.set_margin_end(8);
        if numeric {
            label.add_css_class("numeric-cell");
        }
        group.add_widget(&label);
        label
    }

    fn header(&self, select_all: &gtk::CheckButton) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row.add_css_class("content-table-heading");
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_size_request(28, -1);
        controls.append(&spacer);
        controls.append(select_all);
        self.controls.add_widget(&controls);
        row.append(&controls);
        let name = gtk::Label::new(Some("Title / track"));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_width_chars(12);
        row.append(&name);
        row.append(&self.cell("Type", &self.kind, false));
        row.append(&self.cell("Length", &self.duration, true));
        row.append(&self.cell("Size", &self.size, true));
        row.append(&self.cell("Chapters", &self.chapters, true));
        row
    }
}

fn title_row(
    title: &Title,
    catalog: &MediaCatalog,
    columns: &TitleColumns,
    duplicate_of: Option<&str>,
    expanded: bool,
    on_action: Rc<dyn Fn(ContentAction)>,
    updating_checks: Rc<Cell<bool>>,
) -> (
    gtk::Box,
    gtk::Button,
    gtk::CheckButton,
    Rc<Cell<CheckState>>,
    gtk::Label,
) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("title-row");

    let expander = gtk::Button::builder()
        .icon_name(if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        })
        .has_frame(false)
        .tooltip_text("Show or hide tracks")
        .build();
    expander.add_css_class("title-expander");
    expander.set_valign(gtk::Align::Center);
    expander.set_size_request(28, 28);
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    controls.set_valign(gtk::Align::Center);
    controls.append(&expander);
    let title_id = title.id;
    let callback = on_action.clone();
    expander.connect_clicked(move |_| callback(ContentAction::ToggleExpanded(title_id)));

    let check_state = title_check_state(title);
    let current_check_state = Rc::new(Cell::new(check_state));
    let selected = gtk::CheckButton::new();
    selected.set_active(check_state == CheckState::All);
    selected.set_inconsistent(check_state == CheckState::Some);
    selected.set_tooltip_text(Some(check_state.tooltip()));
    controls.append(&selected);
    let callback = on_action.clone();
    let state = current_check_state.clone();
    selected.connect_toggled(move |_| {
        if updating_checks.get() {
            return;
        }
        callback(ContentAction::SetTitleChecked {
            title_id,
            selected: check_toggle_target(state.get()),
        });
    });

    let image = gtk::Image::from_icon_name("media-optical-symbolic");
    image.set_pixel_size(24);
    controls.append(&image);
    columns.controls.add_widget(&controls);
    row.append(&controls);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    labels.set_valign(gtk::Align::Center);
    let name_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let name = gtk::Label::new(Some(&title.name));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_hexpand(true);
    name.set_width_chars(12);
    name.set_tooltip_text(Some(&title.name));
    name.add_css_class("row-title");
    name_line.append(&name);
    if title.is_main_title {
        let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        badge.add_css_class("main-title-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_tooltip_text(Some(
            "Main title from disc metadata or a dominant DVD playback path",
        ));
        badge.append(&gtk::Label::new(Some("MAIN TITLE")));
        name_line.append(&badge);
    }
    if let Some(original_playlist) = duplicate_of {
        let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        badge.add_css_class("duplicate-title-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_tooltip_text(Some(&format!(
            "This title references the same ordered content ranges as title {original_playlist}",
        )));
        badge.append(&gtk::Label::new(Some(&format!(
            "DUPLICATE OF {original_playlist}",
        ))));
        name_line.append(&badge);
    }
    if let Some(dvd) = &title.dvd
        && !dvd.warnings.is_empty()
    {
        let badge = gtk::Label::new(Some("CHECK DVD LAYOUT"));
        badge.add_css_class("duplicate-title-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_tooltip_text(Some(&dvd.warnings.join(
            "
",
        )));
        name_line.append(&badge);
    }
    labels.append(&name_line);
    row.append(&labels);
    let kind = media_rip_core::model::metadata::MetadataKind::for_title(catalog, title);
    let kind_name = match kind {
        media_rip_core::model::metadata::MetadataKind::Film => "Film",
        media_rip_core::model::metadata::MetadataKind::Television => "Television",
        media_rip_core::model::metadata::MetadataKind::Music => "Music",
        media_rip_core::model::metadata::MetadataKind::Audiobook => "Audiobook",
        media_rip_core::model::metadata::MetadataKind::Video => "Video",
        media_rip_core::model::metadata::MetadataKind::Extras => "Extras",
    };
    row.append(&columns.cell(kind_name, &columns.kind, false));
    row.append(&columns.cell(&format_duration(title.duration_ms), &columns.duration, true));
    let size = title
        .size_bytes
        .map_or_else(|| "Unknown".to_owned(), format_bytes);
    let size_cell = columns.cell(&size, &columns.size, true);
    size_cell.set_tooltip_text(Some(if title.size_is_estimate {
        "Estimated source size from the disc layout; output size depends on format and selected streams."
    } else {
        "Source size; output size depends on format and selected streams."
    }));
    row.append(&size_cell);
    row.append(&columns.cell(&title.chapters.len().to_string(), &columns.chapters, true));

    let click = gtk::GestureClick::new();
    let callback = on_action.clone();
    click.connect_released(move |_, _, _, _| callback(ContentAction::SelectTitle(title_id)));
    row.add_controller(click);
    (row, expander, selected, current_check_state, name)
}

fn duplicate_originals(titles: &[Title]) -> HashMap<u32, String> {
    let mut first_playlist_by_key = HashMap::<&str, &str>::new();
    let mut originals = HashMap::new();
    for title in titles {
        let Some(key) = title.duplicate_key.as_deref() else {
            continue;
        };
        if let Some(original_playlist) = first_playlist_by_key.get(key) {
            originals.insert(title.id, (*original_playlist).to_owned());
        } else {
            first_playlist_by_key.insert(key, &title.source_title_id);
        }
    }
    originals
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckState {
    None,
    Some,
    All,
}

impl CheckState {
    fn tooltip(self) -> &'static str {
        match self {
            Self::None => "This title and all its tracks are excluded",
            Self::Some => "This title is included with selected tracks",
            Self::All => "This title and all its tracks are included",
        }
    }
}

fn title_check_state(title: &Title) -> CheckState {
    if !title.selected {
        return CheckState::None;
    }
    let selected = title.tracks.iter().filter(|track| track.selected).count();
    if title.tracks.is_empty() || selected == title.tracks.len() {
        CheckState::All
    } else {
        CheckState::Some
    }
}

fn select_all_check_state(catalog: &MediaCatalog) -> CheckState {
    if catalog.titles.iter().all(|title| !title.selected) {
        return CheckState::None;
    }
    if catalog
        .titles
        .iter()
        .all(|title| title.selected && title.tracks.iter().all(|track| track.selected))
    {
        CheckState::All
    } else {
        CheckState::Some
    }
}

fn set_check_button_state(button: &gtk::CheckButton, state: CheckState) {
    button.set_active(state == CheckState::All);
    button.set_inconsistent(state == CheckState::Some);
    button.set_tooltip_text(Some(match state {
        CheckState::None => "Select every title and track",
        CheckState::Some | CheckState::All => "Deselect every title and track",
    }));
}

fn check_toggle_target(state: CheckState) -> bool {
    state == CheckState::None
}

fn select_all_checkbox(
    catalog: &MediaCatalog,
    on_action: Rc<dyn Fn(ContentAction)>,
    current_check_state: Rc<Cell<CheckState>>,
    updating_checks: Rc<Cell<bool>>,
) -> gtk::CheckButton {
    let selected = gtk::CheckButton::new();
    selected.set_widget_name("select-all-content");
    selected.update_property(&[gtk::accessible::Property::Label("Select all content")]);
    current_check_state.set(select_all_check_state(catalog));
    set_check_button_state(&selected, current_check_state.get());
    let callback = on_action.clone();
    let check_state = current_check_state.clone();
    selected.connect_toggled(move |_| {
        if !updating_checks.get() {
            callback(ContentAction::SetAllChecked(check_toggle_target(
                check_state.get(),
            )));
        }
    });
    selected
}

fn track_row(
    title_id: u32,
    track: &media_rip_core::Track,
    on_action: Rc<dyn Fn(ContentAction)>,
    updating_checks: Rc<Cell<bool>>,
) -> (gtk::Box, gtk::CheckButton, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("track-row");

    let selected = gtk::CheckButton::new();
    selected.set_active(track.selected);
    selected.set_valign(gtk::Align::Center);
    let stream_index = track.index;
    let callback = on_action.clone();
    selected.connect_toggled(move |button| {
        if updating_checks.get() {
            return;
        }
        callback(ContentAction::SetTrackChecked {
            title_id,
            stream_index,
            selected: button.is_active(),
        });
    });
    row.append(&selected);

    let image = gtk::Image::from_icon_name(track.kind.icon_name());
    image.set_pixel_size(18);
    image.set_valign(gtk::Align::Center);
    row.append(&image);

    let description = gtk::Label::new(Some(&track_description(track)));
    description.set_xalign(0.0);
    description.set_valign(gtk::Align::Center);
    description.set_ellipsize(gtk::pango::EllipsizeMode::End);
    description.set_hexpand(true);
    row.append(&description);

    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| {
        on_action(ContentAction::SelectTrack {
            title_id,
            stream_index,
        });
    });
    row.add_controller(click);
    (row, selected, description)
}

fn track_description(track: &media_rip_core::Track) -> String {
    format!("{}  {}", track.kind.display_name(), track.summary())
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use media_rip_core::{StreamKind, Track};

    use std::{collections::BTreeMap, path::PathBuf};

    use media_rip_core::{DiscKind, MediaCatalog, MediaSource};

    use super::{
        CheckState, check_toggle_target, duplicate_originals, select_all_check_state,
        title_check_state,
    };

    #[test]
    fn partial_checkbox_toggles_directly_to_deselected() {
        assert!(!check_toggle_target(CheckState::Some));
        assert!(check_toggle_target(CheckState::None));
        assert!(!check_toggle_target(CheckState::All));
    }

    #[test]
    fn title_checkbox_reports_none_some_and_all_tracks() {
        let mut title = title_with_tracks(&[false, false]);
        assert_eq!(title_check_state(&title), CheckState::Some);

        title.tracks[0].selected = true;
        assert_eq!(title_check_state(&title), CheckState::Some);

        title.tracks[1].selected = true;
        assert_eq!(title_check_state(&title), CheckState::All);

        title.selected = false;
        assert_eq!(title_check_state(&title), CheckState::None);
    }

    #[test]
    fn select_all_checkbox_reports_top_level_and_nested_selection() {
        let mut catalog = catalog_with_titles(&[&[true, false], &[true]]);
        assert_eq!(select_all_check_state(&catalog), CheckState::Some);

        catalog.titles[0].tracks[1].selected = true;
        assert_eq!(select_all_check_state(&catalog), CheckState::All);

        catalog
            .titles
            .iter_mut()
            .for_each(|title| title.selected = false);
        assert_eq!(select_all_check_state(&catalog), CheckState::None);
    }

    #[test]
    fn only_later_duplicate_playlists_reference_the_first_instance() {
        let mut catalog = catalog_with_titles(&[&[], &[], &[]]);
        catalog.titles[0].duplicate_key = Some("same-playlist".into());
        catalog.titles[1].duplicate_key = Some("same-playlist".into());
        catalog.titles[2].duplicate_key = Some("unique-playlist".into());

        let originals = duplicate_originals(&catalog.titles);

        assert!(!originals.contains_key(&catalog.titles[0].id));
        assert_eq!(
            originals.get(&catalog.titles[1].id).map(String::as_str),
            Some("00001")
        );
        assert!(!originals.contains_key(&catalog.titles[2].id));
    }

    fn catalog_with_titles(selections: &[&[bool]]) -> MediaCatalog {
        MediaCatalog {
            source: MediaSource {
                id: "test".into(),
                name: "Test".into(),
                path: PathBuf::from("test"),
                kind: DiscKind::File,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: selections
                .iter()
                .enumerate()
                .map(|(id, selected)| {
                    let mut title = title_with_tracks(selected);
                    title.id = id as u32;
                    title
                })
                .collect(),
            source_info: Vec::new(),
            cover_art: None,
            track_cover_art: BTreeMap::new(),
            metadata_attributions: Vec::new(),
            audio_metadata_candidates: Vec::new(),
            video_metadata_suggestions: Vec::new(),
            video_episode_context: None,
        }
    }

    fn title_with_tracks(selected: &[bool]) -> media_rip_core::Title {
        media_rip_core::Title {
            dvd: None,
            id: 0,
            source_title_id: "00001".into(),
            disc_title_number: None,
            name: "Playlist 00001".into(),
            is_main_title: false,
            duration_ms: 1_000,
            size_bytes: None,
            size_is_estimate: false,
            chapters: Vec::new(),
            tracks: selected
                .iter()
                .enumerate()
                .map(|(index, selected)| Track {
                    index: index as u32,
                    kind: StreamKind::Audio,
                    codec: "test".into(),
                    codec_long_name: None,
                    language: None,
                    name: None,
                    properties: Vec::new(),
                    selected: *selected,
                })
                .collect(),
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            video_metadata: None,
            output_file: "test.mkv".into(),
            selected: true,
        }
    }
}
