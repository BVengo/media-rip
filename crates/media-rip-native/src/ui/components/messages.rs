use gtk::prelude::*;

#[derive(Debug, Clone)]
pub(crate) struct MessagesPanel {
    pub root: gtk::Box,
    pub view: gtk::TextView,
    toggle: gtk::ToggleButton,
    revealer: gtk::Revealer,
    buffer: gtk::TextBuffer,
}

impl MessagesPanel {
    pub fn new() -> Self {
        let buffer = gtk::TextBuffer::new(None);
        let view = gtk::TextView::builder()
            .buffer(&buffer)
            .editable(false)
            .cursor_visible(false)
            .left_margin(7)
            .right_margin(7)
            .top_margin(5)
            .bottom_margin(5)
            .wrap_mode(gtk::WrapMode::WordChar)
            .build();
        view.add_css_class("message-view");
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(120)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&view)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("activity-panel");
        let toggle = gtk::ToggleButton::with_label("Logs");
        toggle.set_active(false);
        toggle.set_halign(gtk::Align::Start);
        toggle.add_css_class("activity-toggle");
        toggle.set_tooltip_text(Some("Show or hide activity and diagnostic messages"));
        root.append(&toggle);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .reveal_child(false)
            .child(&scroll)
            .build();
        let controlled_revealer = revealer.clone();
        toggle.connect_toggled(move |button| {
            controlled_revealer.set_reveal_child(button.is_active());
        });
        root.append(&revealer);
        root.set_margin_start(8);
        root.set_margin_end(8);
        Self {
            root,
            view,
            toggle,
            revealer,
            buffer,
        }
    }

    pub fn append(&self, message: &str) {
        let mut end = self.buffer.end_iter();
        if self.buffer.char_count() > 0 {
            self.buffer.insert(&mut end, "\n");
        }
        self.buffer.insert(&mut end, message);
        let mut end = self.buffer.end_iter();
        self.view.scroll_to_iter(&mut end, 0.0, false, 0.0, 1.0);
    }

    pub fn clear(&self) {
        self.buffer.set_text("");
    }

    pub fn set_revealed(&self, revealed: bool) {
        self.toggle.set_active(revealed);
        self.revealer.set_reveal_child(revealed);
    }
}
