use gtk::prelude::*;
use media_rip_core::{DiscKind, Drive};

#[derive(Debug, Clone)]
pub(crate) struct SourceBar {
    pub root: gtk::Box,
    pub drives: gtk::ComboBoxText,
    pub open: gtk::Button,
}

impl SourceBar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("source-strip");

        let label = gtk::Label::new(Some("Source"));
        label.add_css_class("source-label");
        root.append(&label);

        let drives = gtk::ComboBoxText::new();
        drives.set_hexpand(true);
        drives.set_tooltip_text(Some("Optical drives and the currently opened source"));
        root.append(&drives);

        let open = gtk::Button::builder()
            .icon_name("media-optical-symbolic")
            .tooltip_text("Open disc")
            .build();
        root.append(&open);
        Self { root, drives, open }
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
