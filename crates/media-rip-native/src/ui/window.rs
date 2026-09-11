use gtk::prelude::*;

use super::components::{
    ContentTree, MessagesPanel, MetadataPanel, OutputReview, Sidebar, SourceBar, Toolbar,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkflowStep {
    Identify,
    Select,
    Metadata,
    Output,
}

#[derive(Debug, Clone)]
pub(crate) struct MainWindow {
    pub window: gtk::ApplicationWindow,
    pub toolbar: Toolbar,
    pub source: SourceBar,
    pub content: ContentTree,
    pub metadata: MetadataPanel,
    pub output_review: OutputReview,
    pub sidebar: Sidebar,
    pub messages: MessagesPanel,
    pub progress_area: gtk::Box,
    pub progress_title: gtk::Label,
    pub progress: gtk::ProgressBar,
    pub progress_detail: gtk::Label,
    pub cancel: gtk::Button,
    pub status: gtk::Label,
    pub workflow_stack: gtk::Stack,
    pub workflow_identify_button: gtk::Button,
    pub workflow_select_button: gtk::Button,
    pub workflow_metadata_button: gtk::Button,
    pub workflow_output_button: gtk::Button,
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

        let workflow = gtk::Grid::new();
        workflow.set_column_homogeneous(true);
        workflow.add_css_class("workflow-bar");
        let workflow_identify_button = workflow_stage("1", "Identify");
        let workflow_select_button = workflow_stage("2", "Select");
        let workflow_metadata_button = workflow_stage("3", "Metadata");
        let workflow_output_button = workflow_stage("4", "Output");
        workflow.attach(&workflow_identify_button, 0, 0, 1, 1);
        workflow.attach(&workflow_select_button, 1, 0, 1, 1);
        workflow.attach(&workflow_metadata_button, 2, 0, 1, 1);
        workflow.attach(&workflow_output_button, 3, 0, 1, 1);
        root.append(&workflow);

        let content = ContentTree::new();
        let sidebar = Sidebar::new();
        let workflow_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .transition_duration(160)
            .hexpand(true)
            .vexpand(true)
            .build();

        let identify_page = workflow_page();
        source.root.set_halign(gtk::Align::Center);
        source.root.set_valign(gtk::Align::Center);
        identify_page.append(&source.root);
        workflow_stack.add_named(&identify_page, Some("identify"));

        content.stack.add_css_class("panel");
        let select_page = workflow_page();
        select_page.append(&content.stack);
        workflow_stack.add_named(&select_page, Some("select"));

        let metadata = MetadataPanel::new();
        let metadata_page = workflow_page();
        metadata_page.append(&metadata.root);
        workflow_stack.add_named(&metadata_page, Some("metadata"));

        let output_review = OutputReview::new();
        let output_page = workflow_page();
        let output_layout = gtk::Paned::new(gtk::Orientation::Horizontal);
        output_layout.add_css_class("output-layout");
        output_layout.set_hexpand(true);
        output_layout.set_vexpand(true);
        output_layout.set_wide_handle(false);
        output_layout.set_resize_start_child(true);
        output_layout.set_shrink_start_child(false);
        output_layout.set_resize_end_child(false);
        output_layout.set_shrink_end_child(false);
        output_layout.set_position(700);
        output_review.root.set_size_request(500, -1);
        output_layout.set_start_child(Some(&output_review.root));
        let output_config = gtk::Box::new(gtk::Orientation::Vertical, 0);
        output_config.add_css_class("output-config");
        output_config.set_size_request(340, -1);
        sidebar.root.set_halign(gtk::Align::Fill);
        sidebar.root.set_vexpand(true);
        output_config.append(&sidebar.root);
        let output_actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
        output_actions.add_css_class("output-actions");
        output_actions.append(&sidebar.selection_summary);
        output_actions.append(&sidebar.rip);
        output_config.append(&output_actions);
        output_layout.set_end_child(Some(&output_config));
        output_page.append(&output_layout);
        workflow_stack.add_named(&output_page, Some("output"));
        root.append(&workflow_stack);
        let messages = MessagesPanel::new();
        root.append(&messages.root);

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
        let cancel = gtk::Button::new();
        cancel.set_valign(gtk::Align::Center);
        let cancel_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let cancel_icon = gtk::Image::from_icon_name("media-playback-stop-symbolic");
        cancel_icon.set_pixel_size(16);
        cancel_icon.set_size_request(16, 16);
        cancel_content.append(&cancel_icon);
        cancel_content.append(&gtk::Label::new(Some("Cancel")));
        cancel.set_child(Some(&cancel_content));
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
            metadata,
            output_review,
            sidebar,
            messages,
            progress_area,
            progress_title,
            progress,
            progress_detail,
            cancel,
            status,
            workflow_stack,
            workflow_identify_button,
            workflow_select_button,
            workflow_metadata_button,
            workflow_output_button,
        }
    }

    pub fn set_workflow_step(&self, step: WorkflowStep) {
        let name = match step {
            WorkflowStep::Identify => "identify",
            WorkflowStep::Select => "select",
            WorkflowStep::Metadata => "metadata",
            WorkflowStep::Output => "output",
        };
        self.workflow_stack.set_visible_child_name(name);
        for (button, button_step) in [
            (&self.workflow_identify_button, WorkflowStep::Identify),
            (&self.workflow_select_button, WorkflowStep::Select),
            (&self.workflow_metadata_button, WorkflowStep::Metadata),
            (&self.workflow_output_button, WorkflowStep::Output),
        ] {
            if button_step == step {
                button.add_css_class("active");
            } else {
                button.remove_css_class("active");
            }
        }
    }

    pub fn set_busy(&self, busy: bool) {
        self.toolbar.open_file.set_sensitive(!busy);
        self.toolbar.open_folder.set_sensitive(!busy);
        self.toolbar.refresh.set_sensitive(!busy);
        self.toolbar.preferences.set_sensitive(!busy);
        self.source.metadata_url.set_sensitive(!busy);
        self.source.identify.set_sensitive(!busy);
        self.sidebar.rip.set_sensitive(!busy);
        self.sidebar.browse_output.set_sensitive(!busy);
        self.sidebar.output_profile.set_sensitive(!busy);
        self.progress_area.set_visible(busy);
        self.cancel.set_sensitive(busy);
        self.workflow_stack.set_sensitive(!busy);
        if busy {
            self.workflow_identify_button.set_sensitive(false);
            self.workflow_select_button.set_sensitive(false);
            self.workflow_metadata_button.set_sensitive(false);
            self.workflow_output_button.set_sensitive(false);
        }
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

fn workflow_stage(number: &str, title: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("workflow-button");
    let stage = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    stage.add_css_class("workflow-stage");
    stage.set_hexpand(true);
    let number = gtk::Label::new(Some(number));
    number.add_css_class("workflow-number");
    number.set_size_request(24, 24);
    number.set_halign(gtk::Align::Center);
    number.set_valign(gtk::Align::Center);
    stage.append(&number);
    let title = gtk::Label::new(Some(title));
    title.set_xalign(0.0);
    title.add_css_class("config-label");
    stage.append(&title);
    button.set_child(Some(&stage));
    button
}

fn workflow_page() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    page.add_css_class("workflow-page");
    page
}
