use gtk::prelude::*;
use media_rip_core::OutputProfile;

use super::InfoPanel;

#[derive(Debug, Clone)]
pub(crate) struct Sidebar {
    pub root: gtk::Box,
    pub info: InfoPanel,
    pub output_entry: gtk::Entry,
    pub browse_output: gtk::Button,
    pub output_profile: gtk::DropDown,
    pub profile_description: gtk::Label,
    pub selection_summary: gtk::Label,
    pub rip: gtk::Button,
    pub rip_label: gtk::Label,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_size_request(300, -1);

        let info = InfoPanel::new();
        info.root.set_vexpand(true);
        root.append(&info.root);

        let profile_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        profile_content.set_margin_top(8);
        profile_content.set_margin_bottom(8);
        profile_content.set_margin_start(8);
        profile_content.set_margin_end(8);
        let profile_names = OutputProfile::ALL.map(OutputProfile::display_name);
        let output_profile = gtk::DropDown::from_strings(&profile_names);
        output_profile.set_hexpand(true);
        output_profile.set_tooltip_text(Some("Choose the output container and track processing"));
        let profile_description = gtk::Label::new(Some(OutputProfile::default().description()));
        profile_description.set_xalign(0.0);
        profile_description.set_wrap(true);
        profile_description.add_css_class("row-detail");
        profile_content.append(&output_profile);
        profile_content.append(&profile_description);
        let profile_panel = super::panel("Format and processing", &profile_content);
        root.append(&profile_panel);

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

        let selection_summary = gtk::Label::new(Some("No items selected"));
        selection_summary.set_xalign(0.5);
        selection_summary.add_css_class("row-detail");
        selection_summary.set_wrap(true);
        root.append(&selection_summary);

        let rip = gtk::Button::new();
        rip.add_css_class("make-mkv-button");
        rip.add_css_class("suggested-action");
        rip.set_sensitive(false);
        let rip_content = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let rip_icon = gtk::Image::from_icon_name("document-save-symbolic");
        rip_icon.set_pixel_size(48);
        let rip_label = gtk::Label::new(Some("Start rip"));
        rip_content.append(&rip_icon);
        rip_content.append(&rip_label);
        rip.set_child(Some(&rip_content));
        root.append(&rip);

        Self {
            root,
            info,
            output_entry,
            browse_output,
            output_profile,
            profile_description,
            selection_summary,
            rip,
            rip_label,
        }
    }
}
