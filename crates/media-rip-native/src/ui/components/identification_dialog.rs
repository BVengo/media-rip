use gtk::prelude::*;

pub(crate) struct IdentificationDialog {
    pub dialog: gtk::Dialog,
    pub query: gtk::Entry,
}

impl IdentificationDialog {
    pub fn new(parent: &gtk::ApplicationWindow, current_query: &str) -> Self {
        let dialog = gtk::Dialog::builder()
            .title("Change identification")
            .transient_for(parent)
            .modal(true)
            .default_width(620)
            .build();
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Find metadata", gtk::ResponseType::Accept);
        crate::ui::space_dialog_actions(
            &dialog,
            &[gtk::ResponseType::Cancel, gtk::ResponseType::Accept],
        );
        dialog.set_default_response(gtk::ResponseType::Accept);
        if let Some(button) = dialog
            .widget_for_response(gtk::ResponseType::Accept)
            .and_then(|widget| widget.downcast::<gtk::Button>().ok())
        {
            button.add_css_class("suggested-action");
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(14);
        content.set_margin_bottom(14);
        content.set_margin_start(16);
        content.set_margin_end(16);
        let heading = gtk::Label::new(Some("What movie or series is this?"));
        heading.set_xalign(0.0);
        heading.add_css_class("preflight-title");
        content.append(&heading);
        let guidance = gtk::Label::new(Some(
            "Enter a title such as The Expanse, or paste a TVMaze or TVDB page for an exact match. Existing track choices will be kept.",
        ));
        guidance.set_xalign(0.0);
        guidance.set_wrap(true);
        guidance.add_css_class("row-detail");
        content.append(&guidance);
        let query = gtk::Entry::builder()
            .text(current_query)
            .placeholder_text("Movie or series title, TVMaze URL, or TVDB URL")
            .hexpand(true)
            .activates_default(true)
            .build();
        content.append(&query);
        let note = gtk::Label::new(Some(
            "MediaRip will present proposed title, episode, tag and filename changes for review before applying them.",
        ));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("row-detail");
        content.append(&note);
        dialog.content_area().append(&content);

        Self { dialog, query }
    }
}
