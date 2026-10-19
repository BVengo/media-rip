use gtk::{gio, prelude::*};

use super::components::{
    ContentTree, MessagesPanel, MetadataPanel, OutputReview, Sidebar, SourceBar, menu_bar,
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
    pub workflow_back: gtk::Button,
    pub workflow_next: gtk::Button,
    page_title: gtk::Label,
    page_hint: gtk::Label,
    source_summary: gtk::Label,

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

        root.append(&menu_bar());
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

        let page_header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        page_header.add_css_class("workflow-header");
        let source_summary = gtk::Label::new(None);
        source_summary.set_xalign(1.0);
        source_summary.set_max_width_chars(28);
        source_summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        source_summary.add_css_class("row-detail");
        source_summary.set_visible(false);
        let page_title = gtk::Label::new(Some("Open a source"));
        page_title.set_xalign(0.0);
        page_title.add_css_class("workflow-card-title");
        let page_hint = gtk::Label::new(Some("Choose a disc, file or folder."));
        page_hint.set_xalign(0.0);
        page_hint.set_wrap(true);
        page_hint.set_hexpand(true);
        page_hint.add_css_class("row-detail");
        page_header.append(&page_title);
        page_header.append(&page_hint);
        page_header.append(&source_summary);
        root.append(&page_header);

        let content = ContentTree::new();
        let sidebar = Sidebar::new();
        let workflow_stack = gtk::Stack::builder()
            .hhomogeneous(false)
            .vhomogeneous(false)
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .transition_duration(160)
            .hexpand(true)
            .vexpand(true)
            .build();

        source.root.set_halign(gtk::Align::Center);
        source.root.set_valign(gtk::Align::Center);
        let source_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&source.root)
            .build();
        source_scroll.add_css_class("workflow-page");
        workflow_stack.add_named(&source_scroll, Some("identify"));

        content.stack.add_css_class("workflow-page");
        workflow_stack.add_named(&content.stack, Some("select"));

        let metadata = MetadataPanel::new();
        metadata.root.add_css_class("workflow-page");
        workflow_stack.add_named(&metadata.root, Some("metadata"));

        let output_review = OutputReview::new();
        let output_layout = gtk::Paned::new(gtk::Orientation::Horizontal);
        output_layout.add_css_class("output-layout");
        output_layout.set_hexpand(true);
        output_layout.set_vexpand(true);
        output_layout.set_wide_handle(false);
        output_layout.set_resize_start_child(true);
        output_layout.set_shrink_start_child(true);
        output_layout.set_resize_end_child(false);
        output_layout.set_shrink_end_child(false);
        output_layout.set_position(700);
        output_review.root.set_size_request(240, -1);
        output_layout.set_start_child(Some(&output_review.root));
        let output_config = gtk::Box::new(gtk::Orientation::Vertical, 0);
        output_config.add_css_class("output-config");
        output_config.set_size_request(300, -1);
        sidebar.root.set_halign(gtk::Align::Fill);
        sidebar.root.set_vexpand(true);
        output_config.append(&sidebar.root);
        let output_actions = gtk::Box::new(gtk::Orientation::Vertical, 8);
        output_actions.add_css_class("output-actions");
        output_actions.append(&sidebar.selection_summary);
        output_actions.append(&sidebar.rip);
        output_config.append(&output_actions);
        output_layout.set_end_child(Some(&output_config));
        output_layout.add_css_class("workflow-page");
        workflow_stack.add_named(&output_layout, Some("output"));
        root.append(&workflow_stack);
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        navigation.add_css_class("workflow-navigation");
        let workflow_back = gtk::Button::with_label("Back");
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        let workflow_next = gtk::Button::with_label("Continue");
        workflow_next.add_css_class("suggested-action");
        let messages = MessagesPanel::new();
        navigation.append(&workflow_back);
        navigation.append(&messages.toggle);
        navigation.append(&spacer);
        navigation.append(&workflow_next);
        root.append(&navigation);

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
        root.append(&messages.root);

        let status = gtk::Label::new(Some("Ready"));
        status.set_xalign(0.0);
        status.add_css_class("status-bar");
        root.append(&status);

        Self {
            window,
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
            workflow_back,
            workflow_next,
            page_title,
            page_hint,
            source_summary,
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
        let (title, hint) = match step {
            WorkflowStep::Identify => ("Open a source", "Choose a disc, file or folder."),
            WorkflowStep::Select => (
                "Choose what to rip",
                "Select items and expand them to choose streams.",
            ),
            WorkflowStep::Metadata => (
                "Review metadata",
                "Review proposed changes or edit current metadata.",
            ),
            WorkflowStep::Output => (
                "Review and rip",
                "Check filenames, format and destination, then rip.",
            ),
        };
        self.workflow_next.set_label(match step {
            WorkflowStep::Identify => "Choose tracks",
            WorkflowStep::Select => "Review metadata",
            WorkflowStep::Metadata => "Review output",
            WorkflowStep::Output => "Continue",
        });
        self.page_title.set_text(title);
        self.page_hint.set_text(hint);
        self.refresh_navigation();
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

    pub fn set_source_summary(&self, catalog: Option<&media_rip_core::MediaCatalog>) {
        use media_rip_core::model::metadata::MetadataKind;
        self.source_summary.set_visible(catalog.is_some());
        if let Some(catalog) = catalog {
            let mut kinds = catalog
                .titles
                .iter()
                .filter(|title| title.selected)
                .map(|title| MetadataKind::for_title(catalog, title));
            let first = kinds.next();
            let uniform = kinds.all(|kind| Some(kind) == first);
            let first = first.filter(|_| uniform);
            let kind = if uniform {
                match first {
                    Some(MetadataKind::Film) => "Film",
                    Some(MetadataKind::Television) => "Television",
                    Some(MetadataKind::Music) => "Music",
                    Some(MetadataKind::Audiobook) => "Audiobook",
                    Some(MetadataKind::Video) => "Video",
                    Some(MetadataKind::Extras) => "Extras",
                    None => "Nothing selected",
                }
            } else {
                "Mixed content"
            };
            let summary = format!(
                "{} · {} · {}",
                catalog.source.name,
                catalog.source.kind.display_name(),
                kind
            );
            self.source_summary.set_text(&summary);
            self.source_summary.set_tooltip_text(Some(&summary));
            if self.workflow_stack.visible_child_name().as_deref() == Some("metadata") {
                self.page_hint.set_text(
                    if first == Some(MetadataKind::Television)
                        && catalog
                            .video_episode_context
                            .as_ref()
                            .is_some_and(|context| !context.choices.is_empty())
                    {
                        "Match disc titles to episodes, then apply metadata."
                    } else if catalog.video_metadata_suggestions.is_empty()
                        && catalog.audio_metadata_candidates.is_empty()
                    {
                        "Edit details, then continue. Changes save as you type."
                    } else {
                        "Apply proposed changes or keep current metadata."
                    },
                );
            }
            if self.workflow_stack.visible_child_name().as_deref() == Some("select") {
                self.page_hint.set_text(match first {
                    Some(MetadataKind::Music) => {
                        "Select tracks to save. Edit album details in Metadata."
                    }
                    Some(MetadataKind::Audiobook) => {
                        "Select audio to save. Edit book details in Metadata."
                    }
                    _ => "Select items and expand them to choose streams.",
                });
            }
        }
    }

    pub fn refresh_navigation(&self) {
        let step = self.workflow_stack.visible_child_name();
        let (back, next, label) = match step.as_deref() {
            Some("select") => (
                Some(&self.workflow_identify_button),
                Some(&self.workflow_metadata_button),
                "Continue to metadata",
            ),
            Some("metadata") => (
                Some(&self.workflow_select_button),
                Some(&self.workflow_output_button),
                "Continue to output",
            ),
            Some("output") => (Some(&self.workflow_metadata_button), None, ""),
            _ => (
                None,
                Some(&self.workflow_select_button),
                "Continue to selection",
            ),
        };
        self.workflow_back.set_visible(back.is_some());
        self.workflow_back
            .set_sensitive(back.is_some_and(|button| button.is_sensitive()));
        let awaiting_metadata =
            step.as_deref() == Some("metadata") && !self.workflow_output_button.is_sensitive();
        self.workflow_next
            .set_visible(next.is_some() && !awaiting_metadata);
        self.workflow_next
            .set_sensitive(next.is_some_and(|button| button.is_sensitive()));
        self.workflow_next.set_label(label);
        self.workflow_next.set_tooltip_text(Some(
            if step.as_deref() == Some("metadata") && !self.workflow_output_button.is_sensitive() {
                "Apply or keep the proposed metadata before continuing."
            } else {
                label
            },
        ));
    }

    pub fn set_busy(&self, busy: bool) {
        for name in ["open-file", "open-folder", "refresh", "preferences"] {
            if let Some(action) = self
                .window
                .lookup_action(name)
                .and_then(|action| action.downcast::<gio::SimpleAction>().ok())
            {
                action.set_enabled(!busy);
            }
        }
        self.source.drives.set_sensitive(!busy);
        self.source.metadata_url.set_sensitive(!busy);
        self.source.identify.set_sensitive(!busy);
        self.sidebar.rip.set_sensitive(!busy);
        self.sidebar.output_entry.set_sensitive(!busy);
        self.sidebar.browse_output.set_sensitive(!busy);
        self.sidebar.output_profile.set_sensitive(!busy);
        self.progress_area.set_visible(busy);
        self.cancel.set_sensitive(busy);
        self.sidebar.info.set_read_only(busy);
        self.content.set_read_only(busy);
        self.metadata.set_read_only(busy);
        self.refresh_navigation();
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
