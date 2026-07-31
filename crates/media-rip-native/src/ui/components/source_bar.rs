use gtk::prelude::*;
use media_rip_core::Drive;

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
            .label("Open disc")
            .icon_name("media-optical-symbolic")
            .build();
        root.append(&open);
        Self { root, drives, open }
    }

    pub fn set_drives(&self, drives: &[Drive]) {
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
            let media = drive
                .media_label
                .as_deref()
                .map(|label| format!(" — {label}"))
                .unwrap_or_default();
            let id_path = drive.mount_path.as_ref().unwrap_or(&drive.device_path);
            self.drives.append(
                Some(&id_path.to_string_lossy()),
                &format!(
                    "{} ({}){media}",
                    drive.display_name,
                    drive.device_path.display()
                ),
            );
        }
        self.drives.set_active(Some(0));
        self.open.set_sensitive(true);
    }

    pub fn show_open_source(&self, path: &std::path::Path, name: &str) {
        self.drives.remove_all();
        self.drives.append(
            Some(&path.to_string_lossy()),
            &format!("{name} — {}", path.display()),
        );
        self.drives.set_active(Some(0));
        self.open.set_sensitive(true);
    }
}
