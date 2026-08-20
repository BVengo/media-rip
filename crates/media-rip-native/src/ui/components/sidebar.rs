use gtk::prelude::*;
use media_rip_core::{DiscKind, OutputProfile};

use super::InfoPanel;

#[derive(Debug, Clone)]
pub(crate) struct Sidebar {
    pub root: gtk::Box,
    pub info: InfoPanel,
    pub output_entry: gtk::Entry,
    pub browse_output: gtk::Button,
    pub output_profile: gtk::DropDown,
    pub profile_panel: gtk::Box,
    pub profile_description: gtk::Label,
    pub selection_summary: gtk::Label,
    pub rip: gtk::Button,
    pub rip_label: gtk::Label,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_size_request(400, -1);
        root.set_margin_end(8);

        let info = InfoPanel::new();

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
        let profile_label = gtk::Label::new(Some("Format and processing"));
        profile_label.set_xalign(0.0);
        profile_label.add_css_class("config-label");
        profile_content.prepend(&profile_label);
        let profile_panel = profile_content;
        profile_panel.set_visible(false);

        let output_content = gtk::Box::new(gtk::Orientation::Vertical, 5);
        output_content.set_margin_top(8);
        output_content.set_margin_bottom(8);
        output_content.set_margin_start(8);
        output_content.set_margin_end(8);
        let output_label = gtk::Label::new(Some("Output folder"));
        output_label.set_xalign(0.0);
        output_label.add_css_class("config-label");
        output_content.append(&output_label);
        let output_row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        let output_entry = gtk::Entry::builder()
            .placeholder_text("Choose an output folder")
            .hexpand(true)
            .build();
        let browse_output = gtk::Button::builder()
            .icon_name("folder-open-symbolic")
            .tooltip_text("Choose output folder")
            .build();
        output_row.append(&output_entry);
        output_row.append(&browse_output);
        output_content.append(&output_row);

        let config_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        config_content.append(&profile_panel);
        config_content.append(&output_content);
        let config_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(110)
            .child(&config_content)
            .build();
        info.add_tab("configuration", "Configuration", &config_scroll);
        info.root.set_vexpand(true);
        root.append(&info.root);

        let selection_summary = gtk::Label::new(Some("No items selected"));
        selection_summary.set_xalign(0.5);
        selection_summary.add_css_class("row-detail");
        selection_summary.set_wrap(true);
        root.append(&selection_summary);

        let rip = gtk::Button::new();
        rip.add_css_class("make-mkv-button");
        rip.add_css_class("suggested-action");
        rip.set_sensitive(false);
        let rip_content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        rip_content.set_halign(gtk::Align::Center);
        let rip_icon = gtk::Image::from_icon_name("document-save-symbolic");
        rip_icon.set_pixel_size(18);
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
            profile_panel,
            profile_description,
            selection_summary,
            rip,
            rip_label,
        }
    }

    pub fn configure_output_profiles(&self, kind: DiscKind, selected: OutputProfile) {
        let profiles = output_profiles_for(kind);
        let names = profiles
            .iter()
            .map(|profile| profile.display_name())
            .collect::<Vec<_>>();
        self.output_profile
            .set_model(Some(&gtk::StringList::new(&names)));
        let selected_index = profiles
            .iter()
            .position(|profile| *profile == selected)
            .unwrap_or_default();
        self.output_profile.set_selected(selected_index as u32);
        let selected = profiles[selected_index];
        self.profile_description.set_text(selected.description());
        self.rip_label.set_text(if selected.is_audio_only() {
            "Rip audio"
        } else {
            "Start rip"
        });
        self.profile_panel.set_visible(true);
    }

    pub fn selected_output_profile(&self, kind: DiscKind) -> OutputProfile {
        let profiles = output_profiles_for(kind);
        profiles
            .get(self.output_profile.selected() as usize)
            .copied()
            .or_else(|| profiles.first().copied())
            .unwrap_or_default()
    }
}

fn output_profiles_for(kind: DiscKind) -> &'static [OutputProfile] {
    const AUDIO_CD_PROFILES: &[OutputProfile] = &[
        OutputProfile::AudioFlac,
        OutputProfile::AudioWav,
        OutputProfile::AudioOpus,
        OutputProfile::AudioAac,
    ];

    if kind == DiscKind::AudioCd {
        AUDIO_CD_PROFILES
    } else {
        &OutputProfile::ALL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_cds_only_offer_audio_profiles() {
        let profiles = output_profiles_for(DiscKind::AudioCd);
        assert_eq!(profiles.len(), 4);
        assert!(profiles.iter().all(|profile| profile.is_audio_only()));
    }

    #[test]
    fn video_sources_keep_audio_extraction_profiles_available() {
        assert_eq!(output_profiles_for(DiscKind::Dvd), &OutputProfile::ALL);
    }
}
