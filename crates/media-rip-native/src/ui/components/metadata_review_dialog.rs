use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use gtk::prelude::*;
use media_rip_core::MediaCatalog;

pub(crate) struct MetadataReviewDialog {
    pub dialog: gtk::Dialog,
    selected_title_ids: Rc<RefCell<BTreeSet<u32>>>,
}

impl MetadataReviewDialog {
    pub fn new(parent: &gtk::ApplicationWindow, catalog: &MediaCatalog) -> Self {
        let selected_title_ids = Rc::new(RefCell::new(
            catalog
                .video_metadata_suggestions
                .iter()
                .map(|suggestion| suggestion.title_id)
                .collect(),
        ));
        let dialog = gtk::Dialog::builder()
            .title("Review metadata changes")
            .transient_for(parent)
            .modal(true)
            .default_width(820)
            .default_height(620)
            .build();
        dialog.add_button("Back", gtk::ResponseType::Cancel);
        dialog.add_button("Apply selected changes", gtk::ResponseType::Accept);
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
        let heading = gtk::Label::new(Some("Choose the proposed changes to apply"));
        heading.set_xalign(0.0);
        heading.add_css_class("preflight-title");
        content.append(&heading);
        let explanation = gtk::Label::new(Some(
            "Only checked rows will change. Names, filenames and output tags can be restored from the Metadata tab afterwards.",
        ));
        explanation.set_xalign(0.0);
        explanation.set_wrap(true);
        explanation.add_css_class("row-detail");
        content.append(&explanation);

        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rows.add_css_class("preflight-files");
        for suggestion in &catalog.video_metadata_suggestions {
            let current_name = catalog
                .titles
                .iter()
                .find(|title| title.id == suggestion.title_id)
                .map(|title| title.name.as_str())
                .unwrap_or("Unknown title");
            rows.append(&suggestion_row(
                suggestion.title_id,
                current_name,
                &suggestion.name,
                suggestion.metadata.attribution.confidence_percent,
                &suggestion.metadata.attribution.provider,
                &suggestion.evidence,
                selected_title_ids.clone(),
            ));
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&rows)
            .build();
        content.append(&scroll);
        dialog.content_area().append(&content);

        Self {
            dialog,
            selected_title_ids,
        }
    }

    pub fn selected_title_ids(&self) -> Vec<u32> {
        self.selected_title_ids.borrow().iter().copied().collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn suggestion_row(
    title_id: u32,
    current_name: &str,
    proposed_name: &str,
    confidence: u8,
    provider: &str,
    evidence: &[String],
    selected_title_ids: Rc<RefCell<BTreeSet<u32>>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    row.add_css_class("metadata-review-row");
    let selected = gtk::CheckButton::new();
    selected.set_active(true);
    selected.set_valign(gtk::Align::Start);
    selected.connect_toggled(move |button| {
        if button.is_active() {
            selected_title_ids.borrow_mut().insert(title_id);
        } else {
            selected_title_ids.borrow_mut().remove(&title_id);
        }
    });
    row.append(&selected);

    let labels = gtk::Box::new(gtk::Orientation::Vertical, 3);
    labels.set_hexpand(true);
    let change = gtk::Label::new(Some(&format!("{current_name}  →  {proposed_name}")));
    change.set_xalign(0.0);
    change.set_wrap(true);
    change.add_css_class("row-title");
    labels.append(&change);
    let detail = gtk::Label::new(Some(&format!("{provider} · match score {confidence}/100")));
    detail.set_xalign(0.0);
    detail.add_css_class("row-detail");
    labels.append(&detail);
    if !evidence.is_empty() {
        let why = gtk::Label::new(Some(&format!("Why: {}", evidence.join("; "))));
        why.set_xalign(0.0);
        why.set_wrap(true);
        why.add_css_class("row-detail");
        labels.append(&why);
    }
    row.append(&labels);
    row
}
