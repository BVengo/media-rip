use gtk::prelude::*;
use media_rip_core::{DiscKind, Drive};

use super::DiscUiState;

#[derive(Debug, Clone)]
pub(crate) struct SourceBar {
    pub root: gtk::Box,
    pub drives: gtk::ComboBoxText,
    pub open: gtk::Button,
    pub metadata_url: gtk::Entry,
    pub identify: gtk::Button,
    pub identity_status: gtk::Label,
    open_icon: gtk::Image,
    open_title: gtk::Label,
    open_detail: gtk::Label,
    metadata_row: gtk::Box,
}

impl SourceBar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        root.add_css_class("source-picker");

        let drives = gtk::ComboBoxText::new();
        drives.set_hexpand(true);
        drives.set_size_request(520, 40);
        drives.add_css_class("source-select");
        drives.set_tooltip_text(Some("Optical drives and the currently opened source"));

        let open = gtk::Button::new();
        open.add_css_class("suggested-action");
        open.add_css_class("source-identify-button");
        open.set_tooltip_text(Some("Identify the selected disc"));
        let open_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        open_content.set_valign(gtk::Align::Center);
        let open_icon = gtk::Image::from_icon_name("media-optical-symbolic");
        open_icon.set_pixel_size(42);
        open_icon.set_valign(gtk::Align::Center);
        let open_labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        open_labels.set_valign(gtk::Align::Center);
        let open_title = gtk::Label::new(Some("No disc detected"));
        open_title.set_xalign(0.0);
        open_title.add_css_class("source-identify-title");
        let open_detail = gtk::Label::new(Some("Insert an optical disc to begin"));
        open_detail.set_xalign(0.0);
        open_detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        open_detail.add_css_class("source-identify-detail");
        open_labels.append(&open_title);
        open_labels.append(&open_detail);
        open_content.append(&open_icon);
        open_content.append(&open_labels);
        open.set_child(Some(&open_content));
        root.append(&drives);
        root.append(&open);

        let metadata_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        metadata_row.add_css_class("identification-row");
        metadata_row.set_hexpand(true);
        metadata_row.set_visible(false);
        let metadata_url = gtk::Entry::new();
        let identity_status = gtk::Label::new(Some("Automatic match"));
        identity_status.set_xalign(0.0);
        identity_status.set_hexpand(true);
        identity_status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        identity_status.add_css_class("row-detail");
        let identify = gtk::Button::with_label("Change identification…");
        identify.set_tooltip_text(Some(
            "Search again using a movie or series title or an exact provider page",
        ));
        metadata_row.append(&identity_status);
        metadata_row.append(&identify);
        root.append(&metadata_row);

        Self {
            root,
            drives,
            open,
            metadata_url,
            identify,
            identity_status,
            open_icon,
            open_title,
            open_detail,
            metadata_row,
        }
    }

    pub fn set_disc_state(&self, state: DiscUiState, enabled: bool) {
        self.open.set_sensitive(enabled);
        match state {
            DiscUiState::Absent => {
                self.open_icon.set_icon_name(Some("media-optical-symbolic"));
                self.open_title.set_text("No disc detected");
                self.open_detail.set_text("Insert an optical disc to begin");
            }
            DiscUiState::Reading => {
                self.open_icon
                    .set_icon_name(Some("content-loading-symbolic"));
                self.open_title.set_text("Reading disc");
                self.open_detail
                    .set_text("Waiting for the operating system to identify the media");
            }
            DiscUiState::Ready { kind, label } => {
                self.open_icon.set_icon_name(Some(kind.icon_name()));
                self.open_title
                    .set_text(&format!("Identify {}", kind.display_name()));
                self.open_detail.set_text(
                    label
                        .as_deref()
                        .unwrap_or("Disc detected and ready to identify"),
                );
            }
        }
    }

    pub fn set_drives(&self, drives: &[Drive]) {
        let selected = self.drives.active_id().map(|id| id.to_string());
        self.drives.remove_all();
        if drives.is_empty() {
            self.drives.append(
                Some(""),
                "No optical drive detected — use Open files or Open folder",
            );
            self.drives.set_active(Some(0));
            self.open.set_sensitive(false);
            return;
        }

        for drive in drives {
            let media = drive_description(drive);
            self.drives.append(
                Some(&drive.device_path.to_string_lossy()),
                &format!(
                    "{} ({}) — {media}",
                    drive.display_name,
                    drive.device_path.display()
                ),
            );
        }
        let restored = selected
            .as_deref()
            .and_then(|id| {
                drives
                    .iter()
                    .position(|drive| drive.device_path.to_string_lossy() == id)
            })
            .map(|index| index as u32);
        self.drives.set_active(Some(restored.unwrap_or(0)));
    }

    pub fn show_open_source(&self, path: &std::path::Path, name: &str) {
        self.drives.remove_all();
        self.drives.append(
            Some(&path.to_string_lossy()),
            &format!("{name} — {}", path.display()),
        );
        self.drives.set_active(Some(0));
    }

    pub fn set_metadata_link_visible(&self, visible: bool) {
        self.metadata_row.set_visible(visible);
    }

    pub fn set_identity_status(&self, status: &str, url: Option<&str>) {
        let display = url.map_or_else(|| status.to_owned(), |url| format!("{status} · {url}"));
        if let Some(url) = url {
            self.identity_status.set_markup(&format!(
                "{} · <a href=\"{}\">{}</a>",
                gtk::glib::markup_escape_text(status),
                gtk::glib::markup_escape_text(url),
                gtk::glib::markup_escape_text(url),
            ));
        } else {
            self.identity_status.set_text(status);
        }
        self.identity_status.set_tooltip_text(Some(&display));
    }
}

fn drive_description(drive: &Drive) -> String {
    if !drive.media_present {
        return "No disc".to_owned();
    }
    let kind = match drive.media_kind {
        DiscKind::Unknown => "Reading disc".to_owned(),
        kind => kind.display_name().to_owned(),
    };
    drive
        .media_label
        .as_deref()
        .filter(|label| !label.is_empty())
        .map(|label| format!("{kind}: {label}"))
        .unwrap_or(kind)
}
