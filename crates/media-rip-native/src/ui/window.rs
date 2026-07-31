use gtk::prelude::*;

use super::components::{ContentTree, MessagesPanel, Sidebar, SourceBar, Toolbar};

#[derive(Debug, Clone)]
pub(crate) struct MainWindow {
    pub window: gtk::ApplicationWindow,
    pub toolbar: Toolbar,
    pub source: SourceBar,
    pub content: ContentTree,
    pub sidebar: Sidebar,
    pub messages: MessagesPanel,
    pub progress_area: gtk::Box,
    pub progress_title: gtk::Label,
    pub progress: gtk::ProgressBar,
    pub progress_detail: gtk::Label,
    pub cancel: gtk::Button,
    pub status: gtk::Label,
}

impl MainWindow {
    pub fn new(application: &gtk::Application) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("MediaRip")
            .default_width(1120)
            .default_height(700)
            .build();

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.set_child(Some(&root));

        let toolbar = Toolbar::new();
        root.append(&toolbar.root);
        let source = SourceBar::new();
        root.append(&source.root);

        let content = ContentTree::new();
        let sidebar = Sidebar::new();
        let working_area = gtk::Paned::new(gtk::Orientation::Horizontal);
        working_area.set_wide_handle(true);
        working_area.set_resize_start_child(true);
        working_area.set_shrink_start_child(true);
        working_area.set_resize_end_child(false);
        working_area.set_shrink_end_child(true);
        let content_panel = super::components::panel("Content", &content.stack);
        content_panel.set_size_request(420, -1);
        working_area.set_start_child(Some(&content_panel));
        working_area.set_end_child(Some(&sidebar.root));
        working_area.set_position(730);

        let messages = MessagesPanel::new();
        let main_pane = gtk::Paned::new(gtk::Orientation::Vertical);
        main_pane.set_wide_handle(true);
        main_pane.set_resize_start_child(true);
        main_pane.set_shrink_start_child(false);
        main_pane.set_resize_end_child(false);
        main_pane.set_shrink_end_child(false);
        main_pane.set_start_child(Some(&working_area));
        main_pane.set_end_child(Some(&messages.root));
        main_pane.set_position(280);
        main_pane.set_vexpand(true);
        root.append(&main_pane);

        let progress_area = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        progress_area.add_css_class("progress-area");
        progress_area.set_visible(false);
        let progress_labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        progress_labels.set_hexpand(true);
        let progress_title = gtk::Label::new(Some("Working…"));
        progress_title.set_xalign(0.0);
        let progress = gtk::ProgressBar::new();
        progress.set_hexpand(true);
        progress.set_show_text(true);
        let progress_detail = gtk::Label::new(None);
        progress_detail.set_xalign(0.0);
        progress_detail.add_css_class("row-detail");
        progress_labels.append(&progress_title);
        progress_labels.append(&progress);
        progress_labels.append(&progress_detail);
        let cancel = gtk::Button::builder()
            .label("Cancel")
            .icon_name("process-stop-symbolic")
            .build();
        progress_area.append(&progress_labels);
        progress_area.append(&cancel);
        root.append(&progress_area);

        let status = gtk::Label::new(Some("Ready"));
        status.set_xalign(0.0);
        status.add_css_class("status-bar");
        root.append(&status);

        Self {
            window,
            toolbar,
            source,
            content,
            sidebar,
            messages,
            progress_area,
            progress_title,
            progress,
            progress_detail,
            cancel,
            status,
        }
    }

    pub fn set_busy(&self, busy: bool) {
        self.toolbar.open_file.set_sensitive(!busy);
        self.toolbar.open_folder.set_sensitive(!busy);
        self.toolbar.open_disc.set_sensitive(!busy);
        self.toolbar.refresh.set_sensitive(!busy);
        self.toolbar.preferences.set_sensitive(!busy);
        self.source.open.set_sensitive(!busy);
        self.content.idle_open.set_sensitive(!busy);
        self.sidebar.rip.set_sensitive(!busy);
        self.sidebar.browse_output.set_sensitive(!busy);
        self.sidebar.output_profile.set_sensitive(!busy);
        self.progress_area.set_visible(busy);
        self.cancel.set_sensitive(busy);
    }

    pub fn set_status(&self, message: &str, class: Option<&str>) {
        self.status.remove_css_class("status-error");
        self.status.remove_css_class("status-success");
        if let Some(class) = class {
            self.status.add_css_class(class);
        }
        self.status.set_text(message);
    }
}
