use std::collections::BTreeSet;

use gtk::prelude::*;
use media_rip_core::{RipRequest, StreamKind, util::format_bytes};

pub(crate) struct PreflightDialog {
    pub dialog: gtk::Dialog,
}

impl PreflightDialog {
    pub fn new(parent: &gtk::ApplicationWindow, request: &RipRequest) -> Self {
        let validation_error = media_rip_engine::validate_rip_request(request)
            .err()
            .map(|error| error.to_string());
        let dialog = gtk::Dialog::builder()
            .title("Review rip")
            .transient_for(parent)
            .modal(true)
            .default_width(760)
            .default_height(620)
            .build();
        dialog.add_button("Back", gtk::ResponseType::Cancel);
        dialog.add_button("Start rip", gtk::ResponseType::Accept);
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
            button.set_sensitive(validation_error.is_none());
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(14);
        content.set_margin_bottom(14);
        content.set_margin_start(16);
        content.set_margin_end(16);

        let heading = gtk::Label::new(Some("Check what MediaRip will create"));
        heading.set_xalign(0.0);
        heading.add_css_class("preflight-title");
        content.append(&heading);

        let summary = request_summary(request);
        let summary_label = gtk::Label::new(Some(&summary));
        summary_label.set_xalign(0.0);
        summary_label.set_wrap(true);
        summary_label.add_css_class("row-detail");
        content.append(&summary_label);

        let destination = labelled_value(
            "Destination",
            &request.output_directory.display().to_string(),
        );
        content.append(&destination);
        content.append(&labelled_value(
            "Format",
            request.output_profile.display_name(),
        ));

        let mut warning_messages = warnings(request);
        if !has_known_blocking_errors(request)
            && let Some(error) = validation_error
        {
            warning_messages.insert(0, format!("Fix required: {error}"));
        }
        if !warning_messages.is_empty() {
            let warning = gtk::Label::new(Some(&warning_messages.join("\n")));
            warning.set_xalign(0.0);
            warning.set_wrap(true);
            warning.add_css_class("preflight-warning");
            content.append(&warning);
        }

        let files_heading = gtk::Label::new(Some("Files and included tracks"));
        files_heading.set_xalign(0.0);
        files_heading.add_css_class("config-label");
        content.append(&files_heading);

        let files = gtk::Box::new(gtk::Orientation::Vertical, 0);
        files.add_css_class("preflight-files");
        for selection in &request.selections {
            let Some(title) = request
                .catalog
                .titles
                .iter()
                .find(|title| title.id == selection.title_id)
            else {
                continue;
            };
            files.append(&file_row(request, title, &selection.output_file));
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&files)
            .build();
        content.append(&scroll);
        dialog.content_area().append(&content);

        Self { dialog }
    }
}

fn request_summary(request: &RipRequest) -> String {
    let titles = request.selections.len();
    let tracks = request
        .selections
        .iter()
        .map(|selection| selection.stream_indices.len())
        .sum::<usize>();
    let known_bytes = request
        .selections
        .iter()
        .filter_map(|selection| {
            request
                .catalog
                .titles
                .iter()
                .find(|title| title.id == selection.title_id)
                .and_then(|title| title.size_bytes)
        })
        .fold(0_u64, u64::saturating_add);
    let size = if known_bytes == 0 {
        "source size unknown".to_owned()
    } else {
        format!("{} source", format_bytes(known_bytes))
    };
    format!("{titles} file(s), {tracks} track(s), {size}")
}

fn warnings(request: &RipRequest) -> Vec<String> {
    let existing = request
        .selections
        .iter()
        .filter(|selection| {
            request
                .output_directory
                .join(&selection.output_file)
                .exists()
        })
        .count();
    let pending_metadata = request
        .selections
        .iter()
        .filter(|selection| {
            request
                .catalog
                .video_metadata_suggestions
                .iter()
                .any(|suggestion| suggestion.title_id == selection.title_id)
        })
        .count();
    let low_confidence = request
        .selections
        .iter()
        .filter_map(|selection| {
            request
                .catalog
                .titles
                .iter()
                .find(|title| title.id == selection.title_id)
        })
        .filter_map(|title| title.video_metadata.as_ref())
        .filter(|metadata| metadata.attribution.confidence_percent < 80)
        .count();
    let mut names = BTreeSet::new();
    let duplicate_names = request
        .selections
        .iter()
        .filter(|selection| !names.insert(selection.output_file.to_lowercase()))
        .count();
    let empty_names = request
        .selections
        .iter()
        .filter(|selection| selection.output_file.trim().is_empty())
        .count();

    let mut warnings = Vec::new();
    if existing > 0 {
        warnings.push(format!(
            "Fix required: {existing} destination file(s) already exist. Go back and change the filename or destination."
        ));
    }
    if duplicate_names > 0 {
        warnings.push(format!(
            "Fix required: {duplicate_names} output filename(s) are duplicated. Go back and give each item a unique filename."
        ));
    }
    if empty_names > 0 {
        warnings.push(format!(
            "Fix required: {empty_names} selected item(s) have no output filename."
        ));
    }
    if pending_metadata > 0 {
        warnings.push(format!(
            "Review recommended: {pending_metadata} selected item(s) still have unapplied metadata suggestions."
        ));
    }
    if low_confidence > 0 {
        warnings.push(format!(
            "Review recommended: {low_confidence} selected item(s) use metadata with a match score below 80."
        ));
    }
    warnings
}

fn has_known_blocking_errors(request: &RipRequest) -> bool {
    let mut names = BTreeSet::new();
    request.selections.iter().any(|selection| {
        selection.output_file.trim().is_empty()
            || !names.insert(selection.output_file.to_lowercase())
            || request
                .output_directory
                .join(&selection.output_file)
                .exists()
    })
}

fn file_row(request: &RipRequest, title: &media_rip_core::Title, output_file: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
    row.add_css_class("preflight-file-row");

    let name = gtk::Label::new(Some(output_file));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_tooltip_text(Some(
        &request
            .output_directory
            .join(output_file)
            .display()
            .to_string(),
    ));
    name.add_css_class("row-title");
    row.append(&name);

    let selected = title
        .tracks
        .iter()
        .filter(|track| track.selected)
        .collect::<Vec<_>>();
    let kinds = [StreamKind::Video, StreamKind::Audio, StreamKind::Subtitle]
        .into_iter()
        .filter_map(|kind| {
            let count = selected.iter().filter(|track| track.kind == kind).count();
            (count > 0).then(|| format!("{count} {}", kind.display_name().to_lowercase()))
        })
        .collect::<Vec<_>>();
    let mut detail = kinds.join(" · ");
    if let Some(size) = title.size_bytes {
        detail.push_str(&format!(" · {}", format_bytes(size)));
    }
    if request.output_directory.join(output_file).exists() {
        detail.push_str(" · already exists");
    }
    let detail = gtk::Label::new(Some(&detail));
    detail.set_xalign(0.0);
    detail.add_css_class("row-detail");
    row.append(&detail);
    row
}

fn labelled_value(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(label));
    label.set_xalign(0.0);
    label.set_width_chars(12);
    label.add_css_class("info-label");
    row.append(&label);
    let value = gtk::Label::new(Some(value));
    value.set_xalign(0.0);
    value.set_wrap(true);
    value.set_selectable(true);
    value.set_hexpand(true);
    row.append(&value);
    row
}
