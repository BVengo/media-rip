use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap},
    rc::Rc,
};

use gtk::{glib, prelude::*};
use media_rip_core::{
    MediaCatalog, Title,
    util::{format_bytes, format_duration},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentAction {
    SelectTitle(u32),
    SelectTrack {
        title_id: u32,
        stream_index: u32,
    },
    ToggleExpanded(u32),
    SetTitleChecked {
        title_id: u32,
        selected: bool,
    },
    SetTrackChecked {
        title_id: u32,
        stream_index: u32,
        selected: bool,
    },
    SetOutputFile {
        title_id: u32,
        output_file: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ContentTree {
    pub stack: gtk::Stack,
    title_box: gtk::Box,
    scroll: gtk::ScrolledWindow,
    pending_scroll_restore: Rc<RefCell<Option<glib::SignalHandlerId>>>,
    expanded_rows: Rc<RefCell<HashMap<u32, ExpandedRow>>>,
    updating_checks: Rc<Cell<bool>>,
    pub idle_open: gtk::Button,
}

#[derive(Debug, Clone)]
struct ExpandedRow {
    button: gtk::Button,
    selected: gtk::CheckButton,
    tracks: gtk::Box,
    track_buttons: HashMap<u32, gtk::CheckButton>,
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
        let image = gtk::Image::from_icon_name("media-optical-symbolic");
        image.set_pixel_size(112);
        let button_label = gtk::Label::new(Some("Open disc"));
        button_label.add_css_class("idle-title");
        button_content.append(&image);
        button_content.append(&button_label);
        idle_open.set_child(Some(&button_content));
        let hint = gtk::Label::new(Some(
            "Insert an optical disc, or open a media file or folder above.",
        ));
        hint.add_css_class("idle-caption");
        idle.append(&idle_open);
        idle.append(&hint);
        stack.add_named(&idle, Some("empty"));

        let title_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        title_box.add_css_class("content-surface");
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
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
            updating_checks: Rc::new(Cell::new(false)),
            idle_open,
        }
    }

    pub fn clear(&self) {
        if let Some(handler) = self.pending_scroll_restore.borrow_mut().take() {
            self.scroll.vadjustment().disconnect(handler);
        }
        self.expanded_rows.borrow_mut().clear();
        clear_box(&self.title_box);
        self.stack.set_visible_child_name("empty");
    }

    pub fn render(
        &self,
        catalog: &MediaCatalog,
        expanded: &BTreeSet<u32>,
        on_action: Rc<dyn Fn(ContentAction)>,
    ) {
        let adjustment = self.scroll.vadjustment();
        let scroll_position = adjustment.value();
        self.expanded_rows.borrow_mut().clear();
        clear_box(&self.title_box);
        for title in &catalog.titles {
            let section = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let (header, button, selected) = title_row(
                title,
                expanded.contains(&title.id),
                on_action.clone(),
                self.updating_checks.clone(),
            );
            section.append(&header);
            let tracks = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let mut track_buttons = HashMap::new();
            for track in &title.tracks {
                let (row, selected) = track_row(
                    title.id,
                    track,
                    on_action.clone(),
                    self.updating_checks.clone(),
                );
                tracks.append(&row);
                track_buttons.insert(track.index, selected);
            }
            tracks.set_visible(expanded.contains(&title.id));
            section.append(&tracks);
            self.title_box.append(&section);
            self.expanded_rows.borrow_mut().insert(
                title.id,
                ExpandedRow {
                    button,
                    selected,
                    tracks,
                    track_buttons,
                },
            );
        }
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
}

fn title_row(
    title: &Title,
    expanded: bool,
    on_action: Rc<dyn Fn(ContentAction)>,
    updating_checks: Rc<Cell<bool>>,
) -> (gtk::Box, gtk::Button, gtk::CheckButton) {
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
    row.append(&expander);
    let title_id = title.id;
    let callback = on_action.clone();
    expander.connect_clicked(move |_| callback(ContentAction::ToggleExpanded(title_id)));

    let check_state = title_check_state(title);
    let selected = gtk::CheckButton::new();
    selected.set_active(check_state == CheckState::All);
    selected.set_inconsistent(check_state == CheckState::Some);
    selected.set_tooltip_text(Some(check_state.tooltip()));
    row.append(&selected);
    let callback = on_action.clone();
    selected.connect_toggled(move |button| {
        if updating_checks.get() {
            return;
        }
        callback(ContentAction::SetTitleChecked {
            title_id,
            selected: button.is_active(),
        });
    });

    let image = gtk::Image::from_icon_name("media-optical-symbolic");
    image.set_pixel_size(24);
    row.append(&image);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_hexpand(true);
    let name_line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let name = gtk::Label::new(Some(&title.name));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_hexpand(true);
    name.add_css_class("row-title");
    name_line.append(&name);
    if title.is_main_title {
        let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        badge.add_css_class("main-title-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_tooltip_text(Some("Main title identified by the Blu-ray disc metadata"));
        badge.append(&gtk::Label::new(Some("MAIN TITLE")));
        name_line.append(&badge);
    }
    if title.duplicate_key.is_some() && !title.is_main_title {
        let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        badge.add_css_class("main-title-badge");
        badge.set_valign(gtk::Align::Center);
        badge.set_tooltip_text(Some(
            "This playlist uses the same authored media ranges as another item",
        ));
        badge.append(&gtk::Label::new(Some("DUPLICATE")));
        name_line.append(&badge);
    }
    let details = [
        format_duration(title.duration_ms),
        format!("{} chapter(s)", title.chapters.len()),
        title.size_bytes.map_or_else(
            || "size unavailable".to_owned(),
            |size| {
                let size = format_bytes(size);
                if title.size_is_estimate {
                    format!("≈ {size}")
                } else {
                    size
                }
            },
        ),
    ]
    .join("  •  ");
    let detail = gtk::Label::new(Some(&details));
    detail.set_xalign(0.0);
    detail.add_css_class("row-detail");
    labels.append(&name_line);
    labels.append(&detail);
    row.append(&labels);

    let output_file = gtk::Entry::builder()
        .text(&title.output_file)
        .width_chars(18)
        .max_width_chars(28)
        .tooltip_text("Output file name")
        .build();
    let callback = on_action.clone();
    output_file.connect_changed(move |entry| {
        callback(ContentAction::SetOutputFile {
            title_id,
            output_file: entry.text().to_string(),
        });
    });
    row.append(&output_file);

    let click = gtk::GestureClick::new();
    let callback = on_action.clone();
    click.connect_released(move |_, _, _, _| callback(ContentAction::SelectTitle(title_id)));
    row.add_controller(click);
    (row, expander, selected)
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
            Self::None => "No tracks are included",
            Self::Some => "Some tracks are included",
            Self::All => "All tracks are included",
        }
    }
}

fn title_check_state(title: &Title) -> CheckState {
    if title.tracks.is_empty() {
        return if title.selected {
            CheckState::All
        } else {
            CheckState::None
        };
    }
    let selected = title.tracks.iter().filter(|track| track.selected).count();
    match selected {
        0 => CheckState::None,
        count if count == title.tracks.len() => CheckState::All,
        _ => CheckState::Some,
    }
}

fn track_row(
    title_id: u32,
    track: &media_rip_core::Track,
    on_action: Rc<dyn Fn(ContentAction)>,
    updating_checks: Rc<Cell<bool>>,
) -> (gtk::Box, gtk::CheckButton) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("track-row");

    let selected = gtk::CheckButton::new();
    selected.set_active(track.selected);
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
    row.append(&image);

    let description = gtk::Label::new(Some(&format!(
        "{}  {}",
        track.kind.display_name(),
        track.summary()
    )));
    description.set_xalign(0.0);
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
    (row, selected)
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use media_rip_core::{StreamKind, Track};

    use super::{CheckState, title_check_state};

    #[test]
    fn title_checkbox_reports_none_some_and_all_tracks() {
        let mut title = title_with_tracks(&[false, false]);
        assert_eq!(title_check_state(&title), CheckState::None);

        title.tracks[0].selected = true;
        assert_eq!(title_check_state(&title), CheckState::Some);

        title.tracks[1].selected = true;
        assert_eq!(title_check_state(&title), CheckState::All);
    }

    fn title_with_tracks(selected: &[bool]) -> media_rip_core::Title {
        media_rip_core::Title {
            id: 0,
            source_title_id: "00001".into(),
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
            output_file: "test.mkv".into(),
            selected: true,
        }
    }
}
