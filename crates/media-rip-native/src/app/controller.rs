use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use gtk::{gio, glib, prelude::*};
use media_rip_core::{
    EngineEvent, LogLevel, MediaEngine, OutputProfile, RipRequest,
    ports::CancellationToken,
    util::{ensure_extension, format_bytes, format_duration, format_rate, sanitize_file_name},
};
use media_rip_engine::{OpenMediaEngine, tool_availability};

use crate::{
    app::{AppSettings, AppState, BusyState, Selection},
    ui::{ContentAction, MainWindow, SettingsDialog, install_styles},
};

#[derive(Clone)]
struct Controller {
    ui: MainWindow,
    state: Rc<RefCell<AppState>>,
    engine: Arc<dyn MediaEngine>,
    events: mpsc::Sender<EngineEvent>,
    worker_events: Rc<RefCell<Option<mpsc::Receiver<EngineEvent>>>>,
}

pub fn launch(application: &gtk::Application, initial_source: Option<PathBuf>) {
    install_styles();
    let ui = MainWindow::new(application);
    let (settings, settings_warning) = match AppSettings::load() {
        Ok(settings) => (settings, None),
        Err(error) => (AppSettings::default(), Some(error)),
    };
    let state = Rc::new(RefCell::new(AppState {
        settings,
        ..AppState::default()
    }));
    let engine: Arc<dyn MediaEngine> = Arc::new(OpenMediaEngine::new());
    let (events, receiver) = mpsc::channel();
    let controller = Controller {
        ui,
        state,
        engine,
        events,
        worker_events: Rc::new(RefCell::new(None)),
    };

    controller.bind_controls();
    controller.install_shortcuts(application);
    controller.bind_window_lifecycle();
    controller.start_event_pump(receiver);
    controller.log_startup();
    if let Some(warning) = settings_warning {
        controller
            .ui
            .messages
            .append(&format!("[warning] Using default settings: {warning}"));
    }
    controller.render();
    controller.ui.window.present();
    if let Some(path) = initial_source {
        controller.start_scan(path);
    } else {
        controller.refresh_drives();
    }
}

impl Controller {
    fn bind_controls(&self) {
        {
            let this = self.clone();
            self.ui
                .toolbar
                .open_file
                .connect_clicked(move |_| this.choose_source(false));
        }
        {
            let this = self.clone();
            self.ui
                .toolbar
                .open_folder
                .connect_clicked(move |_| this.choose_source(true));
        }
        {
            let this = self.clone();
            self.ui
                .toolbar
                .open_disc
                .connect_clicked(move |_| this.open_selected_disc());
        }
        {
            let this = self.clone();
            self.ui
                .source
                .open
                .connect_clicked(move |_| this.open_selected_disc());
        }
        {
            let this = self.clone();
            self.ui
                .content
                .idle_open
                .connect_clicked(move |_| this.open_selected_disc());
        }
        {
            let this = self.clone();
            self.ui
                .toolbar
                .refresh
                .connect_clicked(move |_| this.refresh_drives());
        }
        {
            let this = self.clone();
            self.ui
                .toolbar
                .eject
                .connect_clicked(move |_| this.eject_selected_disc());
        }
        {
            let this = self.clone();
            self.ui
                .toolbar
                .preferences
                .connect_clicked(move |_| this.show_preferences());
        }
        {
            let this = self.clone();
            self.ui
                .sidebar
                .browse_output
                .connect_clicked(move |_| this.choose_output_directory());
        }
        {
            let this = self.clone();
            self.ui
                .sidebar
                .rip
                .connect_clicked(move |_| this.start_rip());
        }
        {
            let this = self.clone();
            self.ui
                .cancel
                .connect_clicked(move |_| this.cancel_active());
        }
        {
            let this = self.clone();
            self.ui
                .sidebar
                .output_entry
                .connect_changed(move |_| this.update_rip_sensitivity());
        }
        {
            let this = self.clone();
            self.ui
                .sidebar
                .output_profile
                .connect_selected_notify(move |_| this.change_output_profile());
        }

        self.install_action("open-file", {
            let this = self.clone();
            move || this.choose_source(false)
        });
        self.install_action("open-folder", {
            let this = self.clone();
            move || this.choose_source(true)
        });
        self.install_action("open-disc", {
            let this = self.clone();
            move || this.open_selected_disc()
        });
        self.install_action("eject", {
            let this = self.clone();
            move || this.eject_selected_disc()
        });
        self.install_action("refresh", {
            let this = self.clone();
            move || this.refresh_drives()
        });
        self.install_action("clear-messages", {
            let this = self.clone();
            move || this.ui.messages.clear()
        });
        self.install_action("preferences", {
            let this = self.clone();
            move || this.show_preferences()
        });
        self.install_action("cancel", {
            let this = self.clone();
            move || this.cancel_active()
        });
        self.install_action("about", {
            let this = self.clone();
            move || this.show_about()
        });

        let quit = gio::SimpleAction::new("quit", None);
        let window = self.ui.window.clone();
        quit.connect_activate(move |_, _| window.close());
        self.ui.window.add_action(&quit);
    }

    fn bind_window_lifecycle(&self) {
        let this = self.clone();
        self.ui.window.connect_close_request(move |_| {
            let mut state = this.state.borrow_mut();
            if !state.is_busy() {
                return glib::Propagation::Proceed;
            }
            state.close_requested = true;
            if let Some(cancellation) = &state.cancellation {
                cancellation.cancel();
            }
            drop(state);
            this.ui.cancel.set_sensitive(false);
            this.ui.set_status("Cancelling before closing…", None);
            this.ui
                .messages
                .append("Close requested; cancelling the active operation.");
            glib::Propagation::Stop
        });
    }

    fn install_action(&self, name: &str, handler: impl Fn() + 'static) {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| handler());
        self.ui.window.add_action(&action);
    }

    fn install_shortcuts(&self, application: &gtk::Application) {
        for (action, accelerators) in [
            ("win.open-file", &["<Primary>o"][..]),
            ("win.open-folder", &["<Primary><Shift>o"][..]),
            ("win.refresh", &["F5"][..]),
            ("win.preferences", &["<Primary>comma"][..]),
            ("win.cancel", &["Escape"][..]),
            ("win.quit", &["<Primary>q"][..]),
        ] {
            application.set_accels_for_action(action, accelerators);
        }
    }

    fn log_startup(&self) {
        self.ui.messages.append(&format!(
            "MediaRip {} started — native GTK interface.",
            env!("CARGO_PKG_VERSION")
        ));
        let tools = tool_availability();
        self.ui.messages.append(&format!(
            "Open-source media backend: ffprobe {}, ffmpeg {}.",
            availability(tools.ffprobe),
            availability(tools.ffmpeg)
        ));
        self.ui
            .messages
            .append("MakeMKV and makemkvcon are neither called nor required.");
    }

    fn refresh_drives(&self) {
        if self.state.borrow().is_busy() {
            return;
        }
        match self.engine.available_drives() {
            Ok(drives) => {
                self.ui.source.set_drives(&drives);
                let message = if drives.is_empty() {
                    "No optical drive detected."
                } else {
                    "Optical drives refreshed."
                };
                self.ui.set_status(message, None);
                self.ui.messages.append(message);
            }
            Err(error) => self.show_error("Could not enumerate optical drives", &error.to_string()),
        }
    }

    fn open_selected_disc(&self) {
        let Some(id) = self.ui.source.drives.active_id() else {
            self.show_error(
                "No optical source",
                "Insert a disc, refresh the drive list, or open a disc folder.",
            );
            return;
        };
        let path = PathBuf::from(id.as_str());
        if path.as_os_str().is_empty() {
            self.show_error(
                "No optical source",
                "Insert a disc, refresh the drive list, or open a disc folder.",
            );
            return;
        }
        self.start_scan(path);
    }

    fn choose_source(&self, directory: bool) {
        if self.state.borrow().is_busy() {
            return;
        }
        let action = if directory {
            gtk::FileChooserAction::SelectFolder
        } else {
            gtk::FileChooserAction::Open
        };
        let dialog = gtk::FileChooserNative::builder()
            .title(if directory {
                "Open media or disc folder"
            } else {
                "Open media file or disc image"
            })
            .transient_for(&self.ui.window)
            .modal(true)
            .action(action)
            .accept_label("Open")
            .cancel_label("Cancel")
            .build();

        if !directory {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Media files and disc images"));
            for pattern in [
                "*.iso", "*.ISO", "*.mkv", "*.mp4", "*.m2ts", "*.mts", "*.vob", "*.avi", "*.mov",
                "*.flac", "*.mp3", "*.m4a", "*.opus", "*.ogg", "*.wav",
            ] {
                filter.add_pattern(pattern);
            }
            dialog.add_filter(&filter);
            let all = gtk::FileFilter::new();
            all.set_name(Some("All files"));
            all.add_pattern("*");
            dialog.add_filter(&all);
        }

        let this = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept
                && let Some(path) = dialog.file().and_then(|file| file.path())
            {
                this.start_scan(path);
            }
            dialog.destroy();
        });
        dialog.show();
    }

    fn start_scan(&self, path: PathBuf) {
        if self.state.borrow().is_busy() {
            return;
        }
        let token = CancellationToken::default();
        {
            let mut state = self.state.borrow_mut();
            state.busy = BusyState::Scanning;
            state.cancellation = Some(token.clone());
        }
        self.ui.set_busy(true);
        self.ui.progress.set_fraction(0.0);
        self.ui.progress.set_text(Some("Scanning…"));
        self.ui.progress_title.set_text("Opening source");
        self.ui
            .progress_detail
            .set_text(&path.display().to_string());
        self.ui.set_status("Opening source…", None);

        let engine = self.engine.clone();
        let (events, receiver) = mpsc::channel();
        *self.worker_events.borrow_mut() = Some(receiver);
        thread::spawn(move || engine.scan(path, events, token));
    }

    fn start_rip(&self) {
        if self.state.borrow().is_busy() {
            return;
        }
        let output_text = self.ui.sidebar.output_entry.text().trim().to_owned();
        if output_text.is_empty() {
            self.show_error(
                "Output folder required",
                "Choose where the selected output files should be saved.",
            );
            return;
        }
        let output_directory = PathBuf::from(output_text);
        let Some(catalog) = self.state.borrow().catalog.clone() else {
            return;
        };
        let output_profile = self.state.borrow().output_profile;
        let request =
            RipRequest::from_catalog_with_profile(catalog, output_directory, false, output_profile);
        if request.selections.is_empty() {
            self.show_error(
                "Nothing selected",
                "Select at least one title and one of its tracks.",
            );
            return;
        }
        if request
            .selections
            .iter()
            .any(|selection| selection.stream_indices.is_empty())
        {
            self.show_error(
                "Title has no selected tracks",
                "Expand each selected item and select at least one track.",
            );
            return;
        }

        let token = CancellationToken::default();
        {
            let mut state = self.state.borrow_mut();
            state.output_directory = request.output_directory.clone();
            state.busy = BusyState::Ripping;
            state.cancellation = Some(token.clone());
        }
        self.ui.set_busy(true);
        self.ui.progress.set_fraction(0.0);
        self.ui.progress.set_text(Some("0%"));
        self.ui.progress_title.set_text("Preparing selected items");
        self.ui.progress_detail.set_text("");
        self.ui.set_status("Saving selected items…", None);

        let engine = self.engine.clone();
        let (events, receiver) = mpsc::channel();
        *self.worker_events.borrow_mut() = Some(receiver);
        thread::spawn(move || engine.rip(request, events, token));
    }

    fn cancel_active(&self) {
        if let Some(cancellation) = &self.state.borrow().cancellation {
            cancellation.cancel();
            self.ui.cancel.set_sensitive(false);
            self.ui.set_status("Cancelling…", None);
            self.ui.messages.append("Cancellation requested.");
        }
    }

    fn start_event_pump(&self, receiver: mpsc::Receiver<EngineEvent>) {
        let this = self.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok(event) = receiver.try_recv() {
                this.handle_event(event);
            }
            this.pump_worker_events();
            glib::ControlFlow::Continue
        });
    }

    fn pump_worker_events(&self) {
        let (events, disconnected) = {
            let worker_events = self.worker_events.borrow();
            let Some(receiver) = worker_events.as_ref() else {
                return;
            };
            drain_available_events(receiver)
        };

        for event in events {
            self.handle_event(event);
        }

        if disconnected {
            *self.worker_events.borrow_mut() = None;
            self.handle_worker_disconnect();
        }
    }

    fn handle_worker_disconnect(&self) {
        let busy = self.state.borrow().busy;
        if busy == BusyState::Idle || self.finish_busy() {
            return;
        }
        let (title, detail) = match busy {
            BusyState::Scanning => (
                "Source scan stopped unexpectedly",
                "The scan worker exited without reporting a result. You can retry opening the source.",
            ),
            BusyState::Ripping => (
                "Save operation stopped unexpectedly",
                "The save worker exited without reporting a result. Existing completed files were left in place.",
            ),
            BusyState::Idle => unreachable!(),
        };
        self.show_error(title, detail);
        self.render();
    }

    fn handle_event(&self, event: EngineEvent) {
        match event {
            EngineEvent::Log { level, message } => {
                self.ui
                    .messages
                    .append(&format!("[{}] {message}", log_label(level)));
            }
            EngineEvent::ScanStarted { source } => {
                self.ui
                    .messages
                    .append(&format!("Opening {}.", source.display()));
            }
            EngineEvent::ScanProgress {
                current_title,
                title_count,
                source_title_id,
            } => {
                self.ui.progress.set_fraction(
                    current_title.saturating_sub(1) as f64 / title_count.max(1) as f64,
                );
                self.ui
                    .progress
                    .set_text(Some(&format!("Title {current_title} of {title_count}")));
                self.ui
                    .progress_title
                    .set_text(&format!("Reading title {source_title_id}"));
            }
            EngineEvent::ScanFinished(catalog) => {
                let mut catalog = *catalog;
                let output_profile =
                    OutputProfile::from_id(&self.state.borrow().settings.advanced.default_profile)
                        .unwrap_or_default();
                self.state
                    .borrow()
                    .settings
                    .apply_catalog_defaults(&mut catalog);
                for title in &mut catalog.titles {
                    title.output_file =
                        ensure_extension(&title.output_file, output_profile.extension());
                }
                let audio_only = catalog.titles.iter().all(|title| {
                    title
                        .tracks
                        .iter()
                        .all(|track| track.kind != media_rip_core::StreamKind::Video)
                });
                let output = default_output_directory(
                    &catalog.source.name,
                    audio_only,
                    &self.state.borrow().settings,
                );
                {
                    let mut state = self.state.borrow_mut();
                    state.expanded_titles.clear();
                    if let Some(title) = catalog.titles.first() {
                        state.selection = Selection::Title(title.id);
                    } else {
                        state.selection = Selection::Source;
                    }
                    state.output_directory = output.clone();
                    state.output_profile = output_profile;
                    state.catalog = Some(catalog);
                }
                if self.finish_busy() {
                    return;
                }
                let state = self.state.borrow();
                if let Some(catalog) = &state.catalog {
                    self.ui
                        .source
                        .show_open_source(&catalog.source.path, &catalog.source.name);
                }
                drop(state);
                self.ui
                    .sidebar
                    .output_entry
                    .set_text(&output.to_string_lossy());
                self.ui.sidebar.output_profile.set_selected(
                    OutputProfile::ALL
                        .iter()
                        .position(|profile| *profile == output_profile)
                        .unwrap_or_default() as u32,
                );
                self.ui
                    .set_status("Source opened successfully.", Some("status-success"));
                self.render();
            }
            EngineEvent::ScanFailed(error) => {
                if self.finish_busy() {
                    return;
                }
                self.show_error("Could not open source", &error);
                self.render();
            }
            EngineEvent::ScanCancelled => {
                if self.finish_busy() {
                    return;
                }
                self.ui.messages.append("Source scan cancelled.");
                self.ui.set_status("Cancelled.", None);
                self.render();
            }
            EngineEvent::RipStarted { title_count } => {
                self.ui
                    .messages
                    .append(&format!("Saving {title_count} selected item(s)."));
            }
            EngineEvent::RipProgress(progress) => {
                self.ui.progress.set_fraction(progress.fraction);
                self.ui
                    .progress
                    .set_text(Some(&format!("{:.0}%", progress.fraction * 100.0)));
                self.ui.progress_title.set_text(&format!(
                    "Saving item {} of {} — {}",
                    progress.current_title, progress.title_count, progress.title_name
                ));
                let mut details = vec![
                    format!(
                        "Elapsed {}",
                        format_duration(progress.elapsed_seconds * 1000)
                    ),
                    format!("{}", progress.destination.display()),
                ];
                if let Some(remaining) = progress.remaining_seconds {
                    details.insert(
                        1,
                        format!("Remaining {}", format_duration(remaining * 1000)),
                    );
                }
                if let Some(rate) = progress.read_rate_bytes {
                    details.insert(1, format_rate(rate));
                }
                if let Some(bytes) = progress.bytes_written {
                    details.insert(1, format_bytes(bytes));
                }
                self.ui.progress_detail.set_text(&details.join("  •  "));
            }
            EngineEvent::TitleFinished {
                title_name,
                destination,
            } => {
                self.ui
                    .messages
                    .append(&format!("Saved {title_name} to {}.", destination.display()));
            }
            EngineEvent::RipFinished {
                saved_titles,
                destination,
            } => {
                if self.finish_busy() {
                    return;
                }
                let message = format!("{saved_titles} item(s) saved to {}.", destination.display());
                self.ui.messages.append(&message);
                self.ui
                    .set_status("All selected items were saved.", Some("status-success"));
                self.show_message("Copy complete", &message, gtk::MessageType::Info);
                self.render();
                if self.state.borrow().settings.io.eject_when_finished {
                    self.eject_selected_disc();
                }
            }
            EngineEvent::RipFailed(error) => {
                if self.finish_busy() {
                    return;
                }
                self.show_error("Could not save selected items", &error);
                self.render();
            }
            EngineEvent::RipCancelled => {
                if self.finish_busy() {
                    return;
                }
                self.ui.messages.append("Save operation cancelled.");
                self.ui.set_status("Cancelled.", None);
                self.render();
            }
        }
    }

    fn finish_busy(&self) -> bool {
        let close_requested = {
            let mut state = self.state.borrow_mut();
            state.busy = BusyState::Idle;
            state.cancellation = None;
            state.close_requested
        };
        self.ui.set_busy(false);
        self.ui.progress_area.set_visible(false);
        if close_requested {
            self.ui.window.close();
        }
        close_requested
    }

    fn handle_content_action(&self, action: ContentAction) {
        let content_changed = matches!(
            action,
            ContentAction::SetTitleChecked { .. } | ContentAction::SetTrackChecked { .. }
        );
        let mut expanded_change = None;
        {
            let mut state = self.state.borrow_mut();
            match action {
                ContentAction::SelectTitle(id) => state.selection = Selection::Title(id),
                ContentAction::SelectTrack {
                    title_id,
                    stream_index,
                } => {
                    state.selection = Selection::Track {
                        title_id,
                        stream_index,
                    };
                }
                ContentAction::ToggleExpanded(id) => {
                    let expanded = if state.expanded_titles.remove(&id) {
                        false
                    } else {
                        state.expanded_titles.insert(id);
                        true
                    };
                    expanded_change = Some((id, expanded));
                }
                ContentAction::SetTitleChecked { title_id, selected } => {
                    if let Some(title) = state.catalog.as_mut().and_then(|catalog| {
                        catalog.titles.iter_mut().find(|item| item.id == title_id)
                    }) {
                        title.selected = selected;
                        for track in &mut title.tracks {
                            track.selected = selected;
                        }
                    }
                    state.selection = Selection::Title(title_id);
                }
                ContentAction::SetTrackChecked {
                    title_id,
                    stream_index,
                    selected,
                } => {
                    if let Some(title) = state.catalog.as_mut().and_then(|catalog| {
                        catalog.titles.iter_mut().find(|item| item.id == title_id)
                    }) {
                        if let Some(track) = title
                            .tracks
                            .iter_mut()
                            .find(|track| track.index == stream_index)
                        {
                            track.selected = selected;
                        }
                        title.selected = title.tracks.iter().any(|track| track.selected);
                    }
                    state.selection = Selection::Track {
                        title_id,
                        stream_index,
                    };
                }
                ContentAction::SetOutputFile {
                    title_id,
                    output_file,
                } => {
                    if let Some(title) = state.catalog.as_mut().and_then(|catalog| {
                        catalog.titles.iter_mut().find(|item| item.id == title_id)
                    }) {
                        title.output_file = output_file;
                    }
                    state.selection = Selection::Title(title_id);
                }
            }
        }
        if let Some((title_id, expanded)) = expanded_change {
            self.ui.content.set_expanded(title_id, expanded);
            return;
        }
        if content_changed {
            let state = self.state.borrow();
            if let Some(title) = state.catalog.as_ref().and_then(|catalog| {
                let title_id = match state.selection {
                    Selection::Title(title_id) | Selection::Track { title_id, .. } => title_id,
                    Selection::Source => return None,
                };
                catalog.titles.iter().find(|title| title.id == title_id)
            }) {
                self.ui.content.sync_title_checks(title);
            }
            self.ui
                .sidebar
                .info
                .render(state.catalog.as_ref(), &state.selection);
            drop(state);
            self.update_rip_sensitivity();
        } else {
            let state = self.state.borrow();
            self.ui
                .sidebar
                .info
                .render(state.catalog.as_ref(), &state.selection);
            drop(state);
            self.update_rip_sensitivity();
        }
    }

    fn render(&self) {
        let callback = {
            let this = self.clone();
            Rc::new(move |action| this.handle_content_action(action)) as Rc<dyn Fn(ContentAction)>
        };
        let state = self.state.borrow();
        if let Some(catalog) = &state.catalog {
            self.ui
                .content
                .render(catalog, &state.expanded_titles, callback);
            self.ui.sidebar.info.render(Some(catalog), &state.selection);
        } else {
            self.ui.content.clear();
            self.ui.sidebar.info.render(None, &Selection::Source);
        }
        drop(state);
        self.update_rip_sensitivity();
    }

    fn update_rip_sensitivity(&self) {
        let state = self.state.borrow();
        let selected_titles = state
            .catalog
            .as_ref()
            .map(|catalog| {
                catalog
                    .titles
                    .iter()
                    .filter(|title| {
                        title.selected && title.tracks.iter().any(|track| track.selected)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let selected = !selected_titles.is_empty();
        if selected {
            let track_count = selected_titles
                .iter()
                .map(|title| title.selected_track_count())
                .sum::<usize>();
            let known_bytes = selected_titles
                .iter()
                .filter_map(|title| title.size_bytes)
                .fold(0_u64, u64::saturating_add);
            let unknown_sizes = selected_titles
                .iter()
                .filter(|title| title.size_bytes.is_none())
                .count();
            let estimated = selected_titles.iter().any(|title| title.size_is_estimate);
            let profile = state.output_profile;
            let size = if profile != OutputProfile::PreserveMkv {
                if unknown_sizes > 0 {
                    "output size unknown".to_owned()
                } else {
                    format!("{} source · output size unknown", format_bytes(known_bytes))
                }
            } else if unknown_sizes > 0 {
                "size partly unknown".to_owned()
            } else if estimated {
                format!("about {}", format_bytes(known_bytes))
            } else {
                format!("up to {}", format_bytes(known_bytes))
            };
            self.ui.sidebar.selection_summary.set_text(&format!(
                "{} item(s) · {track_count} track(s) · {size}",
                selected_titles.len()
            ));
        } else {
            self.ui
                .sidebar
                .selection_summary
                .set_text("No items selected");
        }
        let has_output = !self.ui.sidebar.output_entry.text().trim().is_empty();
        self.ui
            .sidebar
            .rip
            .set_sensitive(!state.is_busy() && selected && has_output);
    }

    fn change_output_profile(&self) {
        if self.state.borrow().is_busy() {
            return;
        }
        let index = self.ui.sidebar.output_profile.selected() as usize;
        let profile = OutputProfile::ALL.get(index).copied().unwrap_or_default();
        {
            let mut state = self.state.borrow_mut();
            state.output_profile = profile;
            if let Some(catalog) = &mut state.catalog {
                for title in &mut catalog.titles {
                    title.output_file = ensure_extension(&title.output_file, profile.extension());
                }
            }
        }
        self.ui
            .sidebar
            .profile_description
            .set_text(profile.description());
        self.ui
            .sidebar
            .rip_label
            .set_text(if profile.is_audio_only() {
                "Rip audio"
            } else {
                "Start rip"
            });
        self.render();
    }

    fn choose_output_directory(&self) {
        if self.state.borrow().is_busy() {
            return;
        }
        let dialog = gtk::FileChooserNative::builder()
            .title("Choose output folder")
            .transient_for(&self.ui.window)
            .modal(true)
            .action(gtk::FileChooserAction::SelectFolder)
            .accept_label("Select")
            .cancel_label("Cancel")
            .build();
        let current = PathBuf::from(self.ui.sidebar.output_entry.text().as_str());
        if current.is_dir() {
            let _ = dialog.set_current_folder(Some(&gio::File::for_path(current)));
        }
        let this = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept
                && let Some(path) = dialog.file().and_then(|file| file.path())
            {
                this.ui
                    .sidebar
                    .output_entry
                    .set_text(&path.to_string_lossy());
            }
            dialog.destroy();
        });
        dialog.show();
    }

    fn eject_selected_disc(&self) {
        if self.state.borrow().is_busy() {
            return;
        }
        let Some(id) = self.ui.source.drives.active_id() else {
            return;
        };
        let path = PathBuf::from(id.as_str());
        let events = self.events.clone();
        thread::spawn(move || {
            let result = Command::new("eject").arg(&path).output();
            let (level, message) = match result {
                Ok(output) if output.status.success() => {
                    (LogLevel::Success, format!("Ejected {}.", path.display()))
                }
                Ok(output) => (
                    LogLevel::Error,
                    format!(
                        "Could not eject {}: {}",
                        path.display(),
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                ),
                Err(error) => (
                    LogLevel::Error,
                    format!("Could not run eject for {}: {error}", path.display()),
                ),
            };
            let _ = events.send(EngineEvent::Log { level, message });
        });
    }

    fn show_preferences(&self) {
        if self.state.borrow().is_busy() {
            return;
        }
        let settings = self.state.borrow().settings.clone();
        let settings_dialog = SettingsDialog::new(&self.ui.window, &settings);
        let form = settings_dialog.form.clone();
        let this = self.clone();
        settings_dialog
            .dialog
            .connect_response(move |dialog, response| {
                if response == gtk::ResponseType::Accept {
                    let settings = form.settings();
                    match settings.save() {
                        Ok(path) => {
                            this.state.borrow_mut().settings = settings;
                            form.mark_applied();
                            this.ui
                                .messages
                                .append(&format!("Settings saved to {}.", path.display()));
                            this.ui
                                .set_status("Settings saved.", Some("status-success"));
                        }
                        Err(error) => {
                            this.show_error("Could not save settings", &error);
                        }
                    }
                    return;
                }
                dialog.close();
            });
        settings_dialog.dialog.present();
    }

    fn show_about(&self) {
        let dialog = gtk::AboutDialog::builder()
            .transient_for(&self.ui.window)
            .modal(true)
            .program_name("MediaRip")
            .version(env!("CARGO_PKG_VERSION"))
            .comments("A native, open-source optical media ripper and transcoder.")
            .license_type(gtk::License::Gpl30)
            .build();
        dialog.set_authors(&["MediaRip contributors"]);
        dialog.present();
    }

    fn show_error(&self, title: &str, detail: &str) {
        self.ui
            .messages
            .append(&format!("[error] {title}: {detail}"));
        self.ui.set_status(title, Some("status-error"));
        self.show_message(title, detail, gtk::MessageType::Error);
    }

    fn show_message(&self, title: &str, detail: &str, message_type: gtk::MessageType) {
        let dialog = gtk::MessageDialog::builder()
            .transient_for(&self.ui.window)
            .modal(true)
            .message_type(message_type)
            .buttons(gtk::ButtonsType::Ok)
            .text(title)
            .secondary_text(detail)
            .build();
        dialog.connect_response(|dialog, _| dialog.close());
        dialog.present();
    }
}

fn drain_available_events(receiver: &mpsc::Receiver<EngineEvent>) -> (Vec<EngineEvent>, bool) {
    let mut events = Vec::new();
    let disconnected = loop {
        match receiver.try_recv() {
            Ok(event) => events.push(event),
            Err(mpsc::TryRecvError::Empty) => break false,
            Err(mpsc::TryRecvError::Disconnected) => break true,
        }
    };
    (events, disconnected)
}

fn availability(available: bool) -> &'static str {
    if available { "available" } else { "missing" }
}

fn log_label(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "info",
        LogLevel::Warning => "warning",
        LogLevel::Error => "error",
        LogLevel::Success => "ok",
    }
}

fn default_output_directory(
    source_name: &str,
    audio_only: bool,
    settings: &AppSettings,
) -> PathBuf {
    if settings.output.destination_mode == "none" {
        return PathBuf::new();
    }
    if settings.output.destination_mode == "custom"
        && !settings.output.custom_directory.trim().is_empty()
    {
        return PathBuf::from(settings.output.custom_directory.trim());
    }
    let xdg_variable = if audio_only {
        "XDG_MUSIC_DIR"
    } else {
        "XDG_VIDEOS_DIR"
    };
    let home_folder = if audio_only { "Music" } else { "Videos" };
    let base = std::env::var_os(xdg_variable)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(home_folder))
                .filter(|path| path.is_dir())
        })
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| Path::new(".").to_owned());
    base.join(sanitize_file_name(source_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_every_time_starts_without_an_output_directory() {
        let mut settings = AppSettings::default();
        settings.output.destination_mode = "none".to_owned();

        assert!(
            default_output_directory("Movie", false, &settings)
                .as_os_str()
                .is_empty()
        );
    }

    #[test]
    fn custom_output_directory_is_honoured() {
        let mut settings = AppSettings::default();
        settings.output.destination_mode = "custom".to_owned();
        settings.output.custom_directory = "/media/archive".to_owned();

        assert_eq!(
            default_output_directory("Movie", false, &settings),
            PathBuf::from("/media/archive")
        );
    }

    #[test]
    fn worker_events_are_drained_before_disconnection_is_reported() {
        let (sender, receiver) = mpsc::channel();
        sender.send(EngineEvent::ScanCancelled).unwrap();
        drop(sender);

        let (events, disconnected) = drain_available_events(&receiver);

        assert_eq!(events, vec![EngineEvent::ScanCancelled]);
        assert!(disconnected);
    }

    #[test]
    fn live_empty_worker_channel_is_not_reported_as_disconnected() {
        let (_sender, receiver) = mpsc::channel();

        let (events, disconnected) = drain_available_events(&receiver);

        assert!(events.is_empty());
        assert!(!disconnected);
    }
}
