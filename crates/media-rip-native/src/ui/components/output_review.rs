use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::prelude::*;
use media_rip_core::{
    RipProgress, RipRequest, RipTitleResult, StreamKind,
    util::{format_bytes, format_duration},
};

#[derive(Debug, Clone)]
struct OutputFileRow {
    container: gtk::Box,
    status: gtk::Label,
    progress: gtk::ProgressBar,
}

#[derive(Debug, Clone)]
pub(crate) struct OutputReview {
    pub root: gtk::Box,
    empty_state: gtk::Label,
    warning: gtk::Label,
    files: gtk::Box,
    job_status: gtk::Label,
    rows: Rc<RefCell<BTreeMap<PathBuf, OutputFileRow>>>,
    job_running: Rc<Cell<bool>>,
}

impl OutputReview {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("output-review");
        let job_status = gtk::Label::new(None);
        job_status.set_xalign(0.0);
        job_status.set_wrap(true);
        job_status.set_visible(false);
        root.append(&job_status);
        let empty_state = gtk::Label::new(Some("Choose an output folder to preview files."));
        empty_state.set_xalign(0.0);
        empty_state.set_wrap(true);
        empty_state.add_css_class("row-detail");
        root.append(&empty_state);
        let warning = gtk::Label::new(None);
        warning.set_xalign(0.0);
        warning.set_wrap(true);
        warning.add_css_class("preflight-warning");
        warning.set_visible(false);
        root.append(&warning);
        let files = gtk::Box::new(gtk::Orientation::Vertical, 0);
        files.add_css_class("preflight-files");
        let scroll = gtk::ScrolledWindow::builder()
            .overlay_scrolling(false)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&files)
            .build();
        root.append(&scroll);
        Self {
            root,
            empty_state,
            warning,
            files,
            job_status,
            rows: Rc::new(RefCell::new(BTreeMap::new())),
            job_running: Rc::new(Cell::new(false)),
        }
    }

    pub fn render(&self, request: Option<&RipRequest>) -> bool {
        if self.job_running.get() || (self.job_status.is_visible() && request.is_none()) {
            return false;
        }
        clear_box(&self.files);
        self.rows.borrow_mut().clear();
        self.job_status.set_visible(false);
        self.empty_state.set_visible(request.is_none());
        let Some(request) = request else {
            self.empty_state
                .set_text("Choose an output folder to preview files.");
            self.warning.set_visible(false);
            return false;
        };
        self.empty_state.set_visible(false);
        let columns = OutputColumns::new();
        self.files.append(&columns.header());
        for selection in &request.selections {
            if let Some(title) = request
                .catalog
                .titles
                .iter()
                .find(|title| title.id == selection.title_id)
            {
                let destination = request.output_directory.join(&selection.output_file);
                let row = file_row(request, title, &selection.output_file, &columns);
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
        self.job_status.set_visible(false);
        self.job_status.remove_css_class("status-error");
        self.job_status.remove_css_class("status-success");
        for row in self.rows.borrow().values() {
            set_row_state(row, "Waiting", 0.0, false, None);
        }
    }

    pub fn update_progress(&self, progress: &RipProgress) {
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
            set_row_state(row, &status, 1.0, false, None);
            row.container.add_css_class("output-file-complete");
        }
    }

    pub fn finish_job(&self, saved_titles: usize, destination: &Path) {
        self.job_status.set_text(&format!(
            "Complete · {saved_titles} file(s) saved to {}",
            destination.display()
        ));
        self.job_status.add_css_class("status-success");
        self.job_status.set_visible(true);
        self.job_running.set(false);
    }

    pub fn interrupt_job(&self, message: &str, failed: bool) {
        self.job_status.set_text(message);
        if failed {
            self.job_status.add_css_class("status-error");
        }
        self.job_status.set_visible(true);
        for row in self.rows.borrow().values() {
            if row.progress.fraction() < 1.0 {
                row.status.set_text(if row.progress.is_visible() {
                    "Not completed"
                } else {
                    "Not started"
                });
                row.progress.set_visible(false);
            }
        }
        self.job_running.set(false);
    }

    pub fn clear_job(&self) {
        self.job_running.set(false);
        self.job_status.set_visible(false);
    }
}

struct OutputColumns {
    duration: gtk::SizeGroup,
    size: gtk::SizeGroup,
    streams: gtk::SizeGroup,
}

impl OutputColumns {
    fn new() -> Self {
        Self {
            duration: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            size: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
            streams: gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal),
        }
    }

    fn cell(&self, text: &str, group: &gtk::SizeGroup) -> gtk::Label {
        let label = gtk::Label::new(Some(text));
        label.set_xalign(1.0);
        label.set_margin_start(12);
        label.add_css_class("numeric-cell");
        group.add_widget(&label);
        label
    }

    fn header(&self) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("output-table-heading");
        let name = gtk::Label::new(Some("Output filename"));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_width_chars(12);
        row.append(&name);
        row.append(&self.cell("Length", &self.duration));
        let size = self.cell("Source size", &self.size);
        size.set_tooltip_text(Some(
            "Output size depends on the format and selected streams.",
        ));
        row.append(&size);
        row.append(&self.cell("V / A / S", &self.streams));
        row
    }
}

fn file_row(
    request: &RipRequest,
    title: &media_rip_core::Title,
    output_file: &str,
    columns: &OutputColumns,
) -> OutputFileRow {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 3);
    row.add_css_class("preflight-file-row");
    let values = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let name = gtk::Label::new(Some(output_file));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    name.set_width_chars(12);
    name.set_hexpand(true);
    name.set_tooltip_text(Some(
        &request
            .output_directory
            .join(output_file)
            .display()
            .to_string(),
    ));
    name.add_css_class("row-title");
    values.append(&name);
    values.append(&columns.cell(&format_duration(title.duration_ms), &columns.duration));
    let size = title
        .size_bytes
        .map_or_else(|| "Unknown".to_owned(), format_bytes);
    let size_cell = columns.cell(&size, &columns.size);
    size_cell.set_tooltip_text(Some(if title.size_is_estimate {
        "Estimated source size from the disc layout; output size depends on format and selected streams."
    } else {
        "Source size; output size depends on format and selected streams."
    }));
    values.append(&size_cell);
    let counts = [StreamKind::Video, StreamKind::Audio, StreamKind::Subtitle].map(|kind| {
        title
            .tracks
            .iter()
            .filter(|track| track.selected && track.kind == kind)
            .count()
    });
    let streams = columns.cell(
        &format!("{} / {} / {}", counts[0], counts[1], counts[2]),
        &columns.streams,
    );
    streams.set_tooltip_text(Some("Selected video / audio / subtitle streams"));
    values.append(&streams);
    row.append(&values);
    if request.output_directory.join(output_file).exists() {
        let existing = gtk::Label::new(Some("Already exists"));
        existing.set_xalign(0.0);
        existing.add_css_class("status-error");
        row.append(&existing);
    }
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
