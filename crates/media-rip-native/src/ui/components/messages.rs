use gtk::prelude::*;

#[derive(Debug, Clone)]
pub(crate) struct MessagesPanel {
    pub root: gtk::Box,
    pub view: gtk::TextView,
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
            .min_content_height(88)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&view)
            .build();
        let root = super::panel("Messages", &scroll);
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_margin_bottom(8);
        Self { root, view, buffer }
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
}
