use gtk::prelude::*;

use super::InfoPanel;

#[derive(Debug, Clone)]
pub(crate) struct Sidebar {
    pub root: gtk::Box,
    pub info: InfoPanel,
    pub output_entry: gtk::Entry,
    pub browse_output: gtk::Button,
    pub rip: gtk::Button,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_size_request(350, -1);

        let info = InfoPanel::new();
        info.root.set_vexpand(true);
        root.append(&info.root);

        let output_content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        output_content.set_margin_top(8);
        output_content.set_margin_bottom(8);
        output_content.set_margin_start(8);
        output_content.set_margin_end(8);
        let output_entry = gtk::Entry::builder()
            .placeholder_text("Choose an output folder")
            .hexpand(true)
            .build();
        let browse_output = gtk::Button::builder()
            .icon_name("folder-open-symbolic")
            .tooltip_text("Choose output folder")
            .build();
        output_content.append(&output_entry);
        output_content.append(&browse_output);
        let output_panel = super::panel("Output folder", &output_content);
        output_panel.add_css_class("output-panel");
        root.append(&output_panel);

        let rip = gtk::Button::new();
        rip.add_css_class("make-mkv-button");
        rip.add_css_class("suggested-action");
        rip.set_sensitive(false);
        let rip_content = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let rip_icon = gtk::Image::from_icon_name("document-save-symbolic");
        rip_icon.set_pixel_size(48);
        let rip_label = gtk::Label::new(Some("Make MKV"));
        rip_content.append(&rip_icon);
        rip_content.append(&rip_label);
        rip.set_child(Some(&rip_content));
        root.append(&rip);

        Self {
            root,
            info,
            output_entry,
            browse_output,
            rip,
        }
    }
}
