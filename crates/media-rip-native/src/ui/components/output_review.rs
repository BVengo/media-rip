use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::prelude::*;
use media_rip_core::{RipProgress, RipRequest, RipTitleResult, StreamKind, util::format_bytes};

#[derive(Debug, Clone)]
struct OutputFileRow {
    container: gtk::Box,
    status: gtk::Label,
    progress: gtk::ProgressBar,
}

#[derive(Debug, Clone)]
pub(crate) struct OutputReview {
    pub root: gtk::Box,
    summary: gtk::Label,
    warning: gtk::Label,
    files: gtk::Box,
    job_status: gtk::Label,
    job_progress: gtk::ProgressBar,
    job_actions: gtk::Box,
    cancel: gtk::Button,
    rows: Rc<RefCell<BTreeMap<PathBuf, OutputFileRow>>>,
    job_running: Rc<Cell<bool>>,
}

impl OutputReview {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("output-review");
        let job_actions = gtk::Box::new(gtk::Orientation::Vertical, 7);
        job_actions.add_css_class("output-job");
        job_actions.set_visible(false);
        let job_heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let job_status = gtk::Label::new(None);
        job_status.set_xalign(0.0);
        job_status.set_hexpand(true);
        job_status.add_css_class("output-job-status");
        let cancel = gtk::Button::with_label("Cancel");
        cancel.set_action_name(Some("win.cancel"));
        cancel.add_css_class("output-cancel");
        job_heading.append(&job_status);
        job_heading.append(&cancel);
        let job_progress = gtk::ProgressBar::new();
        job_progress.set_show_text(true);
        job_actions.append(&job_heading);
        job_actions.append(&job_progress);
        root.append(&job_actions);
        let summary = gtk::Label::new(Some("Choose an output folder to preview the job."));
        summary.set_xalign(0.0);
        summary.set_wrap(true);
        summary.add_css_class("row-detail");
        root.append(&summary);
        let warning = gtk::Label::new(None);
        warning.set_xalign(0.0);
        warning.set_wrap(true);
        warning.add_css_class("preflight-warning");
        warning.set_visible(false);
        root.append(&warning);
        let files = gtk::Box::new(gtk::Orientation::Vertical, 0);
        files.add_css_class("preflight-files");
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&files)
            .build();
        root.append(&scroll);
        Self {
            root,
            summary,
            warning,
            files,
            job_status,
            job_progress,
            job_actions,
            cancel,
            rows: Rc::new(RefCell::new(BTreeMap::new())),
            job_running: Rc::new(Cell::new(false)),
        }
    }

    pub fn render(&self, request: Option<&RipRequest>) -> bool {
        if self.job_running.get() || (self.job_actions.is_visible() && request.is_none()) {
            return false;
        }
        clear_box(&self.files);
        self.rows.borrow_mut().clear();
        self.job_actions.set_visible(false);
        let Some(request) = request else {
            self.summary
                .set_text("Choose an output folder to preview the job.");
            self.warning.set_visible(false);
            return false;
        };
        self.summary.set_text(&format!(
            "{} file(s) · {} track(s) · {} · {}",
            request.selections.len(),
            request
                .selections
                .iter()
                .map(|selection| selection.stream_indices.len())
                .sum::<usize>(),
            request.output_profile.display_name(),
            request.output_directory.display(),
        ));
        for selection in &request.selections {
            if let Some(title) = request
                .catalog
                .titles
                .iter()
                .find(|title| title.id == selection.title_id)
            {
                let destination = request.output_directory.join(&selection.output_file);
                let row = file_row(request, title, &selection.output_file);
                self.files.append(&row.container);
                self.rows.borrow_mut().insert(destination, row);
            }
        }
        let warnings = warnings(request);
        self.warning.set_visible(!warnings.is_empty());
        self.warning.set_text(&warnings.join("\n"));
        !request.selections.is_empty() && !has_blocking_errors(request)
    }

    pub fn start_job(&self, request: &RipRequest) {
        self.render(Some(request));
        self.job_running.set(true);
        self.warning.set_visible(false);
        self.summary.set_visible(false);
        self.job_actions.set_visible(true);
        self.cancel.set_sensitive(true);
        self.job_status.set_text("Preparing selected files");
        self.job_status.remove_css_class("status-error");
        self.job_status.remove_css_class("status-success");
        self.job_progress.set_fraction(0.0);
        self.job_progress.set_text(Some("0% complete"));
        for row in self.rows.borrow().values() {
            set_row_state(row, "Waiting", 0.0, false, None);
        }
    }

    pub fn update_progress(&self, progress: &RipProgress) {
        let completed = progress.current_title.saturating_sub(1) as f64;
        let overall =
            ((completed + progress.fraction) / progress.title_count.max(1) as f64).clamp(0.0, 1.0);
        self.job_status.set_text(&format!(
            "Ripping file {} of {} · {}",
            progress.current_title, progress.title_count, progress.title_name
        ));
        self.job_progress.set_fraction(overall);
        self.job_progress
            .set_text(Some(&format!("{:.0}% complete", overall * 100.0)));

        if let Some(row) = find_row(&self.rows.borrow(), &progress.destination) {
            let detail = progress
                .bytes_written
                .map(|bytes| format!("Ripping · {}", format_bytes(bytes)))
                .unwrap_or_else(|| "Ripping".to_owned());
            set_row_state(
                row,
                &detail,
                progress.fraction,
                true,
                Some(&format!("{:.0}%", progress.fraction * 100.0)),
            );
        }
    }

    pub fn finish_file(&self, result: &RipTitleResult) {
        if let Some(row) = find_row(&self.rows.borrow(), &result.destination) {
            let status = result
                .bytes_written
                .map(|bytes| format!("Complete · {}", format_bytes(bytes)))
                .unwrap_or_else(|| "Complete".to_owned());
            set_row_state(row, &status, 1.0, true, Some("Complete"));
            row.container.add_css_class("output-file-complete");
        }
    }

    pub fn finish_job(&self, saved_titles: usize, destination: &Path) {
        self.job_status.set_text(&format!(
            "Complete · {saved_titles} file(s) saved to {}",
            destination.display()
        ));
        self.job_status.add_css_class("status-success");
        self.job_progress.set_fraction(1.0);
        self.job_progress.set_text(Some("Complete"));
        self.cancel.set_sensitive(false);
        self.job_running.set(false);
    }

    pub fn interrupt_job(&self, message: &str, failed: bool) {
        self.job_status.set_text(message);
        if failed {
            self.job_status.add_css_class("status-error");
        }
        self.cancel.set_sensitive(false);
        for row in self.rows.borrow().values() {
            if row.progress.fraction() < 1.0 && row.progress.is_visible() {
                row.status.set_text("Not completed");
                row.progress.set_text(Some("Interrupted"));
            } else if !row.progress.is_visible() {
                row.status.set_text("Not started");
                row.progress.set_visible(true);
                row.progress.set_text(Some("Not started"));
            }
        }
        self.job_running.set(false);
    }

    pub fn set_cancelling(&self) {
        self.cancel.set_sensitive(false);
        self.job_status.set_text("Cancelling…");
    }

    pub fn clear_job(&self) {
        self.job_running.set(false);
        self.job_actions.set_visible(false);
    }
}

fn file_row(
    request: &RipRequest,
    title: &media_rip_core::Title,
    output_file: &str,
) -> OutputFileRow {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
    row.add_css_class("preflight-file-row");
    let name = gtk::Label::new(Some(output_file));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.add_css_class("row-title");
    row.append(&name);
    let selected = title
        .tracks
        .iter()
        .filter(|track| track.selected)
        .collect::<Vec<_>>();
    let mut details = [StreamKind::Video, StreamKind::Audio, StreamKind::Subtitle]
        .into_iter()
        .filter_map(|kind| {
            let count = selected.iter().filter(|track| track.kind == kind).count();
            (count > 0).then(|| format!("{count} {}", kind.display_name().to_lowercase()))
        })
        .collect::<Vec<_>>();
    if let Some(size) = title.size_bytes {
        details.push(format_bytes(size));
    }
    if request.output_directory.join(output_file).exists() {
        details.push("already exists".to_owned());
    }
    let detail = gtk::Label::new(Some(&details.join(" · ")));
    detail.set_xalign(0.0);
    detail.add_css_class("row-detail");
    row.append(&detail);
    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.add_css_class("output-file-status");
    status.set_visible(false);
    row.append(&status);
    let progress = gtk::ProgressBar::new();
    progress.set_show_text(true);
    progress.set_visible(false);
    row.append(&progress);
    OutputFileRow {
        container: row,
        status,
        progress,
    }
}

fn find_row<'a>(
    rows: &'a BTreeMap<PathBuf, OutputFileRow>,
    destination: &Path,
) -> Option<&'a OutputFileRow> {
    rows.get(destination).or_else(|| {
        destination
            .file_name()
            .and_then(|name| rows.iter().find(|(path, _)| path.file_name() == Some(name)))
            .map(|(_, row)| row)
    })
}

fn set_row_state(
    row: &OutputFileRow,
    status: &str,
    fraction: f64,
    progress_visible: bool,
    progress_text: Option<&str>,
) {
    row.status.set_text(status);
    row.status.set_visible(true);
    row.progress.set_fraction(fraction);
    row.progress.set_visible(progress_visible);
    row.progress.set_text(progress_text);
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
    let mut names = BTreeSet::new();
    let duplicates = request
        .selections
        .iter()
        .filter(|selection| !names.insert(selection.output_file.to_lowercase()))
        .count();
    let mut warnings = Vec::new();
    if existing > 0 {
        warnings.push(format!("{existing} destination file(s) already exist."));
    }
    if duplicates > 0 {
        warnings.push(format!("{duplicates} output filename(s) are duplicated."));
    }
    warnings
}

fn has_blocking_errors(request: &RipRequest) -> bool {
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

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
