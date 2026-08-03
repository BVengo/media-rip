use std::{
    cell::{Cell, RefCell},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use gtk::{gio, glib, prelude::*};
use media_rip_core::{
    CoverArt, DiscKind, Drive, EngineEvent, LogLevel, MediaEngine, OutputProfile, RipRequest,
    ScanOptions, TrackCoverArt,
    ports::CancellationToken,
    util::{ensure_extension, format_bytes, format_duration, format_rate, sanitize_file_name},
};
use media_rip_engine::{OpenMediaEngine, tool_availability};

use crate::{
    app::{AppSettings, AppState, BusyState, Selection, settings::format_audio_cd_file_name},
    ui::{ContentAction, DiscUiState, InfoAction, MainWindow, SettingsDialog, install_styles},
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
    controller.start_drive_watch();
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
                .source
                .drives
                .connect_changed(move |_| this.update_disc_controls());
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
            "Open-source media backend: ffprobe {}, ffmpeg {}, cdparanoia {}.",
            availability(tools.ffprobe),
            availability(tools.ffmpeg),
            availability(tools.cdparanoia)
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
            Ok(drives) => self.handle_drive_snapshot(drives, true),
            Err(error) => self.show_error("Could not enumerate optical drives", &error.to_string()),
        }
    }

    fn start_drive_watch(&self) {
        let (sender, receiver) = mpsc::channel();
        let scan_in_flight = Rc::new(Cell::new(false));
        let this = self.clone();
        glib::timeout_add_local(Duration::from_millis(750), move || {
            while let Ok(result) = receiver.try_recv() {
                scan_in_flight.set(false);
                if let Ok(drives) = result {
                    this.handle_drive_snapshot(drives, false);
                }
            }
            if !scan_in_flight.replace(true) {
                let engine = this.engine.clone();
                let sender = sender.clone();
                thread::spawn(move || {
                    let result = engine.available_drives().map_err(|error| error.to_string());
                    let _ = sender.send(result);
                });
            }
            glib::ControlFlow::Continue
        });
    }

    fn handle_drive_snapshot(&self, drives: Vec<Drive>, announce_refresh: bool) {
        let (previous, opened_device, has_non_disc_catalog) = {
            let state = self.state.borrow();
            let opened_device = state
                .catalog
                .as_ref()
                .and_then(|catalog| catalog.source.drive.as_ref())
                .map(|drive| drive.device_path.clone());
            (
                state.drives.clone(),
                opened_device,
                state.catalog.is_some()
                    && state
                        .catalog
                        .as_ref()
                        .is_some_and(|catalog| catalog.source.drive.is_none()),
            )
        };

        let ejected = opened_device.as_ref().is_some_and(|device| {
            !drives
                .iter()
                .any(|drive| &drive.device_path == device && drive.media_present)
        });
        {
            let mut state = self.state.borrow_mut();
            state.drives.clone_from(&drives);
            if ejected {
                state.catalog = None;
                state.expanded_titles.clear();
                state.selection = Selection::Source;
            }
        }

        if ejected {
            self.ui.source.set_drives(&drives);
            self.ui
                .messages
                .append("Disc ejected; closed its content list.");
            self.ui.set_status("Disc ejected.", None);
            self.render();
        } else if !has_non_disc_catalog && opened_device.is_none() {
            self.ui.source.set_drives(&drives);
        }

        if announce_refresh {
            let message = if drives.is_empty() {
                "No optical drive detected."
            } else if drives.iter().any(|drive| drive.media_present) {
                "Optical drives refreshed; disc detected."
            } else {
                "Optical drives refreshed; no disc detected."
            };
            self.ui.set_status(message, None);
            self.ui.messages.append(message);
        } else if !ejected {
            self.announce_drive_transition(&previous, &drives);
        }
        self.update_disc_controls();

        let state = self.state.borrow();
        let automatic_drive = (!state.is_busy())
            .then(|| {
                automatic_read_candidate(state.auto_read_armed, &previous, &drives, &state.settings)
            })
            .flatten()
            .cloned();
        drop(state);
        if let Some(drive) = automatic_drive {
            let path = drive
                .mount_path
                .clone()
                .unwrap_or_else(|| drive.device_path.clone());
            self.ui.messages.append(&format!(
                "Automatically opening {}.",
                drive.media_kind.display_name()
            ));
            self.start_scan_with_origin(path, false);
        }
    }

    fn announce_drive_transition(&self, previous: &[Drive], drives: &[Drive]) {
        for drive in drives {
            let old = previous
                .iter()
                .find(|old| old.device_path == drive.device_path);
            let was_present = old.is_some_and(|old| old.media_present);
            if drive.media_present && !was_present {
                let message = if drive_is_ready(drive) {
                    format!(
                        "{} detected and ready to open.",
                        drive.media_kind.display_name()
                    )
                } else {
                    "Disc detected; the system is reading it…".to_owned()
                };
                self.ui.set_status(&message, None);
                self.ui.messages.append(&message);
            } else if drive_is_ready(drive) && old.is_some_and(|old| !drive_is_ready(old)) {
                let message = format!("{} is ready to open.", drive.media_kind.display_name());
                self.ui.set_status(&message, Some("status-success"));
                self.ui.messages.append(&message);
            }
        }
    }

    fn selected_drive(&self) -> Option<Drive> {
        let state = self.state.borrow();
        if let Some(device) = state
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.source.drive.as_ref())
            .map(|drive| &drive.device_path)
        {
            return state
                .drives
                .iter()
                .find(|drive| &drive.device_path == device)
                .cloned();
        }
        let selected = self.ui.source.drives.active_id();
        state
            .drives
            .iter()
            .find(|drive| {
                selected
                    .as_deref()
                    .is_some_and(|id| drive.device_path.to_string_lossy() == id)
            })
            .or_else(|| state.drives.first())
            .cloned()
    }

    fn update_disc_controls(&self) {
        let drive = self.selected_drive();
        let busy = self.state.borrow().is_busy();
        let ready = drive.as_ref().is_some_and(drive_is_ready);
        let icon = drive
            .as_ref()
            .filter(|_| ready)
            .map(|drive| drive.media_kind.icon_name())
            .unwrap_or("media-optical-symbolic");
        let ui_state = match drive.as_ref() {
            Some(drive) if drive_is_ready(drive) => DiscUiState::Ready {
                kind: drive.media_kind,
                label: drive.media_label.clone(),
            },
            Some(drive) if drive.media_present => DiscUiState::Reading,
            _ => DiscUiState::Absent,
        };
        let enabled = ready && !busy;
        self.ui.content.set_disc_state(ui_state, enabled);
        self.ui.toolbar.open_disc.set_icon_name(icon);
        self.ui.toolbar.open_disc.set_sensitive(enabled);
        self.ui.source.open.set_icon_name(icon);
        let open_tooltip = if ready {
            drive
                .as_ref()
                .map(|drive| format!("Open {}", drive.media_kind.display_name()))
                .unwrap_or_else(|| "Open disc".to_owned())
        } else if drive.as_ref().is_some_and(|drive| drive.media_present) {
            "Reading disc…".to_owned()
        } else {
            "Open disc".to_owned()
        };
        self.ui.source.open.set_tooltip_text(Some(&open_tooltip));
        self.ui.source.open.set_sensitive(enabled);
        self.ui
            .toolbar
            .eject
            .set_sensitive(!busy && drive.as_ref().is_some_and(|drive| drive.media_present));
        if let Some(action) = self
            .ui
            .window
            .lookup_action("open-disc")
            .and_then(|action| action.downcast::<gio::SimpleAction>().ok())
        {
            action.set_enabled(enabled);
        }
    }

    fn open_selected_disc(&self) {
        let Some(drive) = self.selected_drive().filter(drive_is_ready) else {
            self.show_error(
                "No optical source",
                "Insert a disc and wait for the system to finish reading it, or open a disc folder.",
            );
            return;
        };
        let path = drive.mount_path.unwrap_or(drive.device_path);
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
        self.start_scan_with_origin(path, true);
    }

    fn start_scan_with_origin(&self, path: PathBuf, user_initiated: bool) {
        if self.state.borrow().is_busy() {
            return;
        }
        let token = CancellationToken::default();
        {
            let mut state = self.state.borrow_mut();
            state.busy = BusyState::Scanning;
            state.cancellation = Some(token.clone());
            if user_initiated {
                state.auto_read_armed = true;
            }
        }
        self.ui.set_busy(true);
        self.update_disc_controls();
        self.ui.sidebar.profile_panel.set_visible(false);
        self.ui.progress.set_fraction(0.0);
        self.ui.progress.set_text(Some("Scanning…"));
        self.ui.progress_title.set_text("Opening source");
        self.ui
            .progress_detail
            .set_text(&path.display().to_string());
        self.ui.set_status("Opening source…", None);

        let engine = self.engine.clone();
        let settings = &self.state.borrow().settings;
        let options = ScanOptions {
            auto_tag_audio: settings.audio_cd.auto_tagging,
            auto_tag_video: settings.integrations.video_metadata_enabled,
            allow_online_access: settings.general.allow_online_access,
            tmdb_access_token: (!settings.integrations.tmdb_access_token.trim().is_empty())
                .then(|| settings.integrations.tmdb_access_token.trim().to_owned()),
        };
        let (events, receiver) = mpsc::channel();
        *self.worker_events.borrow_mut() = Some(receiver);
        thread::spawn(move || engine.scan(path, options, events, token));
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
            state.completed_titles_this_job = 0;
        }
        self.ui.set_busy(true);
        self.update_disc_controls();
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
        // Remove the receiver while its events are handled. A terminal scan
        // event can immediately start an automatic save and install a new
        // receiver; keeping the disconnected scan receiver in this slot would
        // make the cleanup below discard the save receiver instead.
        let Some(receiver) = self.worker_events.borrow_mut().take() else {
            return;
        };
        let (events, disconnected) = drain_available_events(&receiver);

        if !disconnected {
            *self.worker_events.borrow_mut() = Some(receiver);
        }

        for event in events {
            self.handle_event(event);
        }

        if disconnected && self.worker_events.borrow().is_none() {
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
                let output_profile = if catalog.source.kind == media_rip_core::DiscKind::AudioCd {
                    match self.state.borrow().settings.audio_cd.output_format.as_str() {
                        "wav" => OutputProfile::AudioWav,
                        "opus" => OutputProfile::AudioOpus,
                        "aac" => OutputProfile::AudioAac,
                        _ => OutputProfile::AudioFlac,
                    }
                } else {
                    OutputProfile::from_id(&self.state.borrow().settings.advanced.default_profile)
                        .unwrap_or_default()
                };
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
                    catalog.source.kind,
                    audio_only,
                    &self.state.borrow().settings,
                );
                let should_auto_save = catalog.source.drive.is_some()
                    && self
                        .state
                        .borrow()
                        .settings
                        .automation
                        .for_kind(catalog.source.kind)
                        .is_some_and(|automation| automation.save_automatically);
                let metadata_required = catalog.source.kind == DiscKind::AudioCd
                    && self
                        .state
                        .borrow()
                        .settings
                        .automation
                        .require_audio_cd_metadata;
                let metadata_available = audio_cd_metadata_available(&catalog);
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
                let kind = self
                    .state
                    .borrow()
                    .catalog
                    .as_ref()
                    .expect("scan result was just stored")
                    .source
                    .kind;
                self.ui
                    .sidebar
                    .configure_output_profiles(kind, output_profile);
                self.ui
                    .set_status("Source opened successfully.", Some("status-success"));
                self.render();
                if should_auto_save && metadata_required && !metadata_available {
                    self.ui.messages.append(
                        "Automatic save skipped because the Audio CD has no album metadata.",
                    );
                    self.ui.set_status(
                        "Audio CD opened; automatic save is waiting for metadata.",
                        None,
                    );
                } else if should_auto_save {
                    self.ui.messages.append("Starting automatic save.");
                    self.start_rip();
                }
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
            EngineEvent::TitleFinished(result) => {
                let size = result
                    .bytes_written
                    .map(|bytes| format!(" ({})", format_bytes(bytes)))
                    .unwrap_or_default();
                self.ui.messages.append(&format!(
                    "Saved {} to {}{size}.",
                    result.title_name,
                    result.destination.display(),
                ));
                let mut state = self.state.borrow_mut();
                if let Some(catalog) = &mut state.catalog
                    && mark_title_completed(catalog, result.title_id)
                {
                    state.completed_titles_this_job += 1;
                }
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
                let completed = self.state.borrow().completed_titles_this_job;
                if self.finish_busy() {
                    return;
                }
                let detail = resumable_failure_detail(&error, completed);
                self.show_error("Could not save selected items", &detail);
                self.render();
            }
            EngineEvent::RipCancelled => {
                let completed = self.state.borrow().completed_titles_this_job;
                if self.finish_busy() {
                    return;
                }
                let message = if completed == 0 {
                    "Save operation cancelled.".to_owned()
                } else {
                    format!(
                        "Save operation cancelled after {completed} item(s). Unfinished items remain selected; start again to resume."
                    )
                };
                self.ui.messages.append(&message);
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
        self.update_disc_controls();
        self.ui.progress_area.set_visible(false);
        if close_requested {
            self.ui.window.close();
        }
        close_requested
    }

    fn handle_content_action(&self, action: ContentAction) {
        let content_changed = matches!(
            action,
            ContentAction::CycleSelectAll
                | ContentAction::ApplySuggestion(_)
                | ContentAction::ApplyAllSuggestions
                | ContentAction::SetTitleChecked { .. }
                | ContentAction::SetTrackChecked { .. }
        );
        let mut all_checks_changed = false;
        let mut expanded_change = None;
        let mut suggestions_applied = 0;
        {
            let mut state = self.state.borrow_mut();
            let output_extension = state.output_profile.extension();
            match action {
                ContentAction::CycleSelectAll => {
                    if let Some(catalog) = state.catalog.as_mut() {
                        cycle_catalog_selection(catalog);
                        all_checks_changed = true;
                    }
                }
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
                ContentAction::ApplySuggestion(title_id) => {
                    if let Some(catalog) = state.catalog.as_mut()
                        && apply_video_suggestion(catalog, title_id, output_extension)
                    {
                        suggestions_applied = 1;
                    }
                }
                ContentAction::ApplyAllSuggestions => {
                    if let Some(catalog) = state.catalog.as_mut() {
                        let title_ids = catalog
                            .video_metadata_suggestions
                            .iter()
                            .map(|suggestion| suggestion.title_id)
                            .collect::<Vec<_>>();
                        for title_id in title_ids {
                            suggestions_applied += usize::from(apply_video_suggestion(
                                catalog,
                                title_id,
                                output_extension,
                            ));
                        }
                    }
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
            }
        }
        if let Some((title_id, expanded)) = expanded_change {
            self.ui.content.set_expanded(title_id, expanded);
            return;
        }
        if suggestions_applied > 0 {
            self.ui.messages.append(&format!(
                "Applied {suggestions_applied} probable episode metadata suggestion(s)."
            ));
            self.ui.set_status(
                "Probable episode metadata applied; review names before ripping.",
                Some("status-success"),
            );
            self.render();
            return;
        }
        {
            let state = self.state.borrow();
            self.ui.content.sync_view_selection(&state.selection);
        }
        if content_changed {
            let state = self.state.borrow();
            if all_checks_changed {
                if let Some(catalog) = state.catalog.as_ref() {
                    self.ui.content.sync_all_checks(catalog);
                }
            } else if let Some(title) = state.catalog.as_ref().and_then(|catalog| {
                let title_id = match state.selection {
                    Selection::Title(title_id) | Selection::Track { title_id, .. } => title_id,
                    Selection::Source => return None,
                };
                catalog.titles.iter().find(|title| title.id == title_id)
            }) {
                self.ui.content.sync_title_checks(title);
                if let Some(catalog) = state.catalog.as_ref() {
                    self.ui.content.sync_select_all(catalog);
                }
            }
            drop(state);
            self.render_info();
            self.update_rip_sensitivity();
        } else {
            self.render_info();
            self.update_rip_sensitivity();
        }
    }

    fn handle_info_action(&self, action: InfoAction) {
        if let InfoAction::ChooseCoverArt { title_id } = &action {
            self.choose_cover_art(*title_id);
            return;
        }
        let mut state = self.state.borrow_mut();
        let file_name_template = state.settings.audio_cd.file_name_template.clone();
        let output_extension = state.output_profile.extension();
        let Some(catalog) = state.catalog.as_mut() else {
            return;
        };
        let rerender = match action {
            InfoAction::SetTag {
                title_id,
                key,
                value,
                all_tracks,
            } => {
                set_tag(
                    catalog,
                    title_id,
                    &key,
                    &value,
                    all_tracks,
                    &file_name_template,
                    output_extension,
                );
                false
            }
            InfoAction::AddTag {
                title_id,
                key,
                value,
                all_tracks,
            } => {
                let key = key.trim();
                if key.is_empty() {
                    return;
                }
                set_tag(
                    catalog,
                    title_id,
                    key,
                    value.trim(),
                    all_tracks,
                    &file_name_template,
                    output_extension,
                );
                true
            }
            InfoAction::SetOutputFile {
                title_id,
                output_file,
            } => {
                if let Some(title) = catalog.titles.iter_mut().find(|title| title.id == title_id) {
                    title.output_file = output_file.trim().to_owned();
                }
                false
            }
            InfoAction::RemoveCoverArt { title_id: None } => {
                remove_cover_art(catalog, None);
                true
            }
            InfoAction::RemoveCoverArt {
                title_id: Some(title_id),
            } => {
                remove_cover_art(catalog, Some(title_id));
                true
            }
            InfoAction::ChooseCoverArt { .. } => unreachable!(),
        };
        drop(state);
        if rerender {
            self.render_info();
        }
    }

    fn choose_cover_art(&self, title_id: Option<u32>) {
        let is_audio_cd = self
            .state
            .borrow()
            .catalog
            .as_ref()
            .is_some_and(|catalog| catalog.source.kind == DiscKind::AudioCd);
        if !is_audio_cd || self.state.borrow().is_busy() {
            return;
        }
        let dialog = gtk::FileChooserNative::builder()
            .title(if title_id.is_some() {
                "Choose track cover artwork"
            } else {
                "Choose album artwork"
            })
            .transient_for(&self.ui.window)
            .modal(true)
            .action(gtk::FileChooserAction::Open)
            .accept_label("Use image")
            .cancel_label("Cancel")
            .build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("JPEG and PNG images"));
        for mime_type in ["image/jpeg", "image/png"] {
            filter.add_mime_type(mime_type);
        }
        dialog.add_filter(&filter);

        let this = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept
                && let Some(path) = dialog.file().and_then(|file| file.path())
            {
                match cover_art_from_file(&path) {
                    Ok(cover_art) => {
                        if let Some(catalog) = &mut this.state.borrow_mut().catalog {
                            set_cover_art(catalog, title_id, cover_art);
                        }
                        this.ui
                            .messages
                            .append(&format!("Using {} as cover artwork.", path.display()));
                        this.render_info();
                    }
                    Err(error) => this.show_error("Could not use cover artwork", &error),
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }

    fn render_info(&self) {
        let callback = {
            let this = self.clone();
            Rc::new(move |action| this.handle_info_action(action)) as Rc<dyn Fn(InfoAction)>
        };
        let state = self.state.borrow();
        self.ui
            .sidebar
            .info
            .render(state.catalog.as_ref(), &state.selection, callback);
    }

    fn render(&self) {
        let callback = {
            let this = self.clone();
            Rc::new(move |action| this.handle_content_action(action)) as Rc<dyn Fn(ContentAction)>
        };
        let state = self.state.borrow();
        self.ui
            .sidebar
            .profile_panel
            .set_visible(state.catalog.is_some() && state.busy != BusyState::Scanning);
        if let Some(catalog) = &state.catalog {
            self.ui
                .content
                .render(catalog, &state.expanded_titles, &state.selection, callback);
        } else {
            self.ui.content.clear();
        }
        drop(state);
        self.render_info();
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
        let Some(kind) = self
            .state
            .borrow()
            .catalog
            .as_ref()
            .map(|catalog| catalog.source.kind)
        else {
            return;
        };
        let profile = self.ui.sidebar.selected_output_profile(kind);
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
        let Some(drive) = self.selected_drive().filter(|drive| drive.media_present) else {
            return;
        };
        let path = drive.device_path;
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

fn drive_is_ready(drive: &Drive) -> bool {
    drive.media_present && (drive.media_kind != DiscKind::Unknown || drive.mount_path.is_some())
}

fn automatic_read_candidate<'a>(
    armed: bool,
    previous: &[Drive],
    drives: &'a [Drive],
    settings: &AppSettings,
) -> Option<&'a Drive> {
    if !armed {
        return None;
    }
    drives.iter().find(|drive| {
        let newly_ready = drive_is_ready(drive)
            && !previous
                .iter()
                .any(|old| old.device_path == drive.device_path && drive_is_ready(old));
        newly_ready
            && settings
                .automation
                .for_kind(drive.media_kind)
                .is_some_and(|automation| automation.read_automatically)
    })
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

fn set_tag(
    catalog: &mut media_rip_core::MediaCatalog,
    title_id: u32,
    key: &str,
    value: &str,
    all_tracks: bool,
    file_name_template: &str,
    output_extension: &str,
) -> bool {
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return false;
    }
    let album = catalog.source.name.clone();
    let is_audio_cd = catalog.source.kind == DiscKind::AudioCd;
    let mut renamed = false;
    for title in &mut catalog.titles {
        if title.id != title_id && !all_tracks {
            continue;
        }
        let had_auto_name = is_audio_cd
            && title.output_file
                == ensure_extension(
                    &format_audio_cd_file_name(file_name_template, title, &album),
                    output_extension,
                );
        let mut updated = false;
        title.tags.retain_mut(|tag| {
            if !tag.key.eq_ignore_ascii_case(&key) {
                return true;
            }
            if updated {
                return false;
            }
            tag.key.clone_from(&key);
            tag.value = value.to_owned();
            updated = true;
            true
        });
        if !updated {
            title.tags.push(media_rip_core::MediaTag::new(&key, value));
        }
        if had_auto_name {
            let updated_output_file = ensure_extension(
                &format_audio_cd_file_name(file_name_template, title, &album),
                output_extension,
            );
            if title.output_file != updated_output_file {
                title.output_file = updated_output_file;
                renamed = true;
            }
        }
    }
    renamed
}

fn apply_video_suggestion(
    catalog: &mut media_rip_core::MediaCatalog,
    title_id: u32,
    output_extension: &str,
) -> bool {
    let Some(position) = catalog
        .video_metadata_suggestions
        .iter()
        .position(|suggestion| suggestion.title_id == title_id)
    else {
        return false;
    };
    let Some(title_index) = catalog
        .titles
        .iter()
        .position(|title| title.id == title_id && title.video_metadata.is_none())
    else {
        return false;
    };
    let suggestion = catalog.video_metadata_suggestions.remove(position);
    let mut output_file = ensure_extension(&sanitize_file_name(&suggestion.name), output_extension);
    if catalog.titles.iter().enumerate().any(|(index, title)| {
        index != title_index && title.output_file.eq_ignore_ascii_case(&output_file)
    }) {
        output_file = ensure_extension(
            &format!(
                "{}_{}",
                sanitize_file_name(&suggestion.name),
                sanitize_file_name(&catalog.titles[title_index].source_title_id)
            ),
            output_extension,
        );
    }
    let title = &mut catalog.titles[title_index];
    title.name = suggestion.name;
    title.output_file = output_file;
    title.video_metadata = Some(suggestion.metadata);
    for suggested_tag in suggestion.tags {
        if let Some(tag) = title
            .tags
            .iter_mut()
            .find(|tag| tag.key.eq_ignore_ascii_case(&suggested_tag.key))
        {
            *tag = suggested_tag;
        } else {
            title.tags.push(suggested_tag);
        }
    }
    let remaining = catalog.video_metadata_suggestions.len();
    if let Some(field) = catalog
        .source_info
        .iter_mut()
        .find(|field| field.label == "Probable episode matches")
    {
        field.value =
            format!("{remaining} suggestion(s), not applied — review them in the content list");
    }
    if remaining == 0 {
        catalog
            .source_info
            .retain(|field| field.label != "Probable episode matches");
    }
    true
}

fn set_cover_art(
    catalog: &mut media_rip_core::MediaCatalog,
    title_id: Option<u32>,
    cover_art: CoverArt,
) {
    if let Some(title_id) = title_id {
        if catalog.cover_art.as_ref() == Some(&cover_art) {
            catalog.track_cover_art.remove(&title_id);
        } else {
            catalog
                .track_cover_art
                .insert(title_id, TrackCoverArt::Custom(cover_art));
        }
    } else {
        catalog.track_cover_art.retain(|_, track_art| {
            !matches!(track_art, TrackCoverArt::Custom(custom) if custom == &cover_art)
        });
        catalog.cover_art = Some(cover_art);
    }
}

fn remove_cover_art(catalog: &mut media_rip_core::MediaCatalog, title_id: Option<u32>) {
    if let Some(title_id) = title_id {
        catalog
            .track_cover_art
            .insert(title_id, TrackCoverArt::Removed);
    } else {
        catalog.cover_art = None;
    }
}

fn mark_title_completed(catalog: &mut media_rip_core::MediaCatalog, title_id: u32) -> bool {
    let Some(title) = catalog.titles.iter_mut().find(|title| title.id == title_id) else {
        return false;
    };
    title.selected = false;
    for track in &mut title.tracks {
        track.selected = false;
    }
    true
}

fn cycle_catalog_selection(catalog: &mut media_rip_core::MediaCatalog) {
    let any_title_selected = catalog.titles.iter().any(|title| title.selected);
    let everything_selected = !catalog.titles.is_empty()
        && catalog
            .titles
            .iter()
            .all(|title| title.selected && title.tracks.iter().all(|track| track.selected));

    if everything_selected {
        for title in &mut catalog.titles {
            title.selected = false;
        }
    } else if any_title_selected {
        for title in &mut catalog.titles {
            title.selected = true;
            for track in &mut title.tracks {
                track.selected = true;
            }
        }
    } else {
        for title in &mut catalog.titles {
            title.selected = true;
        }
    }
}

fn resumable_failure_detail(error: &str, completed: usize) -> String {
    if completed == 0 {
        return error.to_owned();
    }
    format!(
        "{error}\n\n{completed} item(s) were completed and left in place. Unfinished items remain selected; start again to resume."
    )
}

fn default_output_directory(
    source_name: &str,
    source_kind: DiscKind,
    audio_only: bool,
    settings: &AppSettings,
) -> PathBuf {
    let type_directory = match source_kind {
        DiscKind::AudioCd => &settings.output.audio_cd_directory,
        DiscKind::Dvd => &settings.output.dvd_directory,
        DiscKind::BluRay => &settings.output.bluray_directory,
        _ => "",
    };
    if !type_directory.trim().is_empty() {
        return PathBuf::from(type_directory.trim());
    }
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

fn audio_cd_metadata_available(catalog: &media_rip_core::MediaCatalog) -> bool {
    catalog.source.kind != DiscKind::AudioCd
        || catalog
            .source_info
            .iter()
            .any(|field| field.label == "Tag source" && field.value != "Generic track information")
}

fn cover_art_from_file(path: &Path) -> Result<CoverArt, String> {
    const MAX_COVER_ART_BYTES: u64 = 20 * 1024 * 1024;
    let size = fs::metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?
        .len();
    if size > MAX_COVER_ART_BYTES {
        return Err("The selected image is larger than 20 MB.".to_owned());
    }
    let data =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mime_type =
        cover_art_mime_type(&data).ok_or_else(|| "Choose a valid JPEG or PNG image.".to_owned())?;
    gdk_pixbuf::Pixbuf::from_read(Cursor::new(data.clone()))
        .map_err(|error| format!("the selected image could not be decoded: {error}"))?;
    Ok(CoverArt::new(mime_type, data))
}

fn cover_art_mime_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_every_time_starts_without_an_output_directory() {
        let mut settings = AppSettings::default();
        settings.output.destination_mode = "none".to_owned();

        assert!(
            default_output_directory("Movie", DiscKind::Dvd, false, &settings)
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
            default_output_directory("Movie", DiscKind::Dvd, false, &settings),
            PathBuf::from("/media/archive")
        );
    }

    #[test]
    fn media_type_output_directory_overrides_the_global_default() {
        let mut settings = AppSettings::default();
        settings.output.destination_mode = "custom".to_owned();
        settings.output.custom_directory = "/media/all".to_owned();
        settings.output.audio_cd_directory = "/media/music".to_owned();

        assert_eq!(
            default_output_directory("Album", DiscKind::AudioCd, true, &settings),
            PathBuf::from("/media/music")
        );
        assert_eq!(
            default_output_directory("Movie", DiscKind::Dvd, false, &settings),
            PathBuf::from("/media/all")
        );
    }

    #[test]
    fn generic_audio_cd_track_names_do_not_count_as_metadata() {
        use media_rip_core::{InfoField, MediaCatalog, MediaSource};

        let mut catalog = MediaCatalog {
            source: MediaSource {
                id: "cd".to_owned(),
                name: "Audio CD".to_owned(),
                path: PathBuf::from("/dev/sr0"),
                kind: DiscKind::AudioCd,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: Vec::new(),
            source_info: vec![InfoField::new("Tag source", "Generic track information")],
            cover_art: None,
            track_cover_art: Default::default(),
            metadata_attributions: Vec::new(),
            video_metadata_suggestions: Vec::new(),
        };

        assert!(!audio_cd_metadata_available(&catalog));
        catalog.source_info[0].value = "CD-TEXT".to_owned();
        assert!(audio_cd_metadata_available(&catalog));
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

    #[test]
    fn open_disc_waits_for_inserted_media_to_be_identified_or_mounted() {
        let mut drive = Drive::empty("/dev/sr0", "Test drive");
        assert!(!drive_is_ready(&drive));

        drive.media_present = true;
        assert!(!drive_is_ready(&drive));

        drive.media_kind = DiscKind::Dvd;
        assert!(drive_is_ready(&drive));

        drive.media_kind = DiscKind::Unknown;
        drive.mount_path = Some(PathBuf::from("/media/disc"));
        assert!(drive_is_ready(&drive));
    }

    #[test]
    fn automatic_read_is_armed_by_the_first_user_read() {
        let mut settings = AppSettings::default();
        settings.automation.dvd.read_automatically = true;
        let mut drive = Drive::empty("/dev/sr0", "Test drive");
        drive.media_present = true;
        drive.media_kind = DiscKind::Dvd;
        let drives = vec![drive];

        assert!(automatic_read_candidate(false, &[], &drives, &settings).is_none());
        assert!(automatic_read_candidate(true, &[], &drives, &settings).is_some());
    }

    #[test]
    fn completed_title_is_removed_from_a_resumable_selection() {
        use media_rip_core::{DiscKind, MediaCatalog, MediaSource, StreamKind, Title, Track};

        let title = Title {
            id: 7,
            source_title_id: "7".to_owned(),
            disc_title_number: None,
            name: "Feature".to_owned(),
            is_main_title: true,
            duration_ms: 1_000,
            size_bytes: Some(1_024),
            size_is_estimate: false,
            chapters: Vec::new(),
            tracks: vec![Track {
                index: 0,
                kind: StreamKind::Video,
                codec: "h264".to_owned(),
                codec_long_name: None,
                language: None,
                name: None,
                properties: Vec::new(),
                selected: true,
            }],
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            video_metadata: None,
            output_file: "feature.mkv".to_owned(),
            selected: true,
        };
        let mut catalog = MediaCatalog {
            source: MediaSource {
                id: "source".to_owned(),
                name: "Disc".to_owned(),
                path: PathBuf::from("disc"),
                kind: DiscKind::File,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: vec![title],
            source_info: Vec::new(),
            cover_art: None,
            track_cover_art: Default::default(),
            metadata_attributions: Vec::new(),
            video_metadata_suggestions: Vec::new(),
        };

        assert!(mark_title_completed(&mut catalog, 7));
        assert!(!catalog.titles[0].selected);
        assert!(catalog.titles[0].tracks.iter().all(|track| !track.selected));
        assert!(!mark_title_completed(&mut catalog, 99));
    }

    #[test]
    fn select_all_cycles_without_losing_preserved_track_choices() {
        use media_rip_core::{DiscKind, MediaCatalog, MediaSource, StreamKind, Title, Track};

        let track = |index, selected| Track {
            index,
            kind: StreamKind::Audio,
            codec: "test".to_owned(),
            codec_long_name: None,
            language: None,
            name: None,
            properties: Vec::new(),
            selected,
        };
        let mut catalog = MediaCatalog {
            source: MediaSource {
                id: "test".to_owned(),
                name: "Test".to_owned(),
                path: PathBuf::from("test"),
                kind: DiscKind::File,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: vec![Title {
                id: 0,
                source_title_id: "0".to_owned(),
                disc_title_number: None,
                name: "Test".to_owned(),
                is_main_title: false,
                duration_ms: 1_000,
                size_bytes: None,
                size_is_estimate: false,
                chapters: Vec::new(),
                tracks: vec![track(0, true), track(1, false)],
                segment_count: None,
                segment_map: None,
                duplicate_key: None,
                tags: Vec::new(),
                video_metadata: None,
                output_file: "test.mkv".to_owned(),
                selected: false,
            }],
            source_info: Vec::new(),
            cover_art: None,
            track_cover_art: Default::default(),
            metadata_attributions: Vec::new(),
            video_metadata_suggestions: Vec::new(),
        };

        cycle_catalog_selection(&mut catalog);
        assert!(catalog.titles[0].selected);
        assert_eq!(
            catalog.titles[0]
                .tracks
                .iter()
                .map(|track| track.selected)
                .collect::<Vec<_>>(),
            [true, false]
        );

        cycle_catalog_selection(&mut catalog);
        assert!(catalog.titles[0].tracks.iter().all(|track| track.selected));

        cycle_catalog_selection(&mut catalog);
        assert!(!catalog.titles[0].selected);
        assert!(catalog.titles[0].tracks.iter().all(|track| track.selected));
    }

    #[test]
    fn partial_failure_explains_how_to_resume() {
        assert_eq!(resumable_failure_detail("disk error", 0), "disk error");
        let detail = resumable_failure_detail("disk error", 2);
        assert!(detail.contains("2 item(s) were completed"));
        assert!(detail.contains("start again to resume"));
    }

    #[test]
    fn tags_can_be_added_to_one_or_all_audio_tracks() {
        use media_rip_core::{DiscKind, MediaCatalog, MediaSource, StreamKind, Title, Track};

        let title = |id| Title {
            id,
            source_title_id: (id + 1).to_string(),
            disc_title_number: None,
            name: format!("Track {}", id + 1),
            is_main_title: false,
            duration_ms: 1_000,
            size_bytes: None,
            size_is_estimate: false,
            chapters: Vec::new(),
            tracks: vec![Track {
                index: 0,
                kind: StreamKind::Audio,
                codec: "PCM".to_owned(),
                codec_long_name: None,
                language: None,
                name: None,
                properties: Vec::new(),
                selected: true,
            }],
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            video_metadata: None,
            output_file: format!("{}.flac", id + 1),
            selected: true,
        };
        let mut catalog = MediaCatalog {
            source: MediaSource {
                id: "cd".to_owned(),
                name: "Album".to_owned(),
                path: PathBuf::from("/dev/cdrom"),
                kind: DiscKind::AudioCd,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: vec![title(0), title(1)],
            source_info: Vec::new(),
            cover_art: None,
            track_cover_art: Default::default(),
            metadata_attributions: Vec::new(),
            video_metadata_suggestions: Vec::new(),
        };

        set_tag(
            &mut catalog,
            0,
            "composer",
            "Composer",
            false,
            "{track:02} - {title}",
            "flac",
        );
        assert_eq!(catalog.titles[0].tags[0].value, "Composer");
        assert!(catalog.titles[1].tags.is_empty());

        set_tag(
            &mut catalog,
            0,
            "artist",
            "Various Artists",
            true,
            "{track:02} - {title}",
            "flac",
        );
        assert!(catalog.titles.iter().all(|title| {
            title
                .tags
                .contains(&media_rip_core::MediaTag::new("artist", "Various Artists"))
        }));

        set_tag(
            &mut catalog,
            1,
            "artist",
            "Soloist",
            false,
            "{track:02} - {title}",
            "flac",
        );
        assert_eq!(
            catalog.titles[1]
                .tags
                .iter()
                .find(|tag| tag.key == "artist")
                .map(|tag| tag.value.as_str()),
            Some("Soloist")
        );

        catalog.titles[0]
            .tags
            .push(media_rip_core::MediaTag::new("Album", "Old name"));
        catalog.titles[0]
            .tags
            .push(media_rip_core::MediaTag::new("album", "Duplicate"));
        set_tag(
            &mut catalog,
            0,
            "ALBUM",
            "Canonical name",
            false,
            "{track:02} - {title}",
            "flac",
        );
        let album_tags = catalog.titles[0]
            .tags
            .iter()
            .filter(|tag| tag.key.eq_ignore_ascii_case("album"))
            .collect::<Vec<_>>();
        assert_eq!(album_tags.len(), 1);
        assert_eq!(album_tags[0].key, "album");
        assert_eq!(album_tags[0].value, "Canonical name");

        catalog.titles[0]
            .tags
            .push(media_rip_core::MediaTag::new("title", "Track 1"));
        catalog.titles[0]
            .tags
            .push(media_rip_core::MediaTag::new("track", "1"));
        catalog.titles[0].output_file = "01 - Track 1.opus".to_owned();
        assert!(set_tag(
            &mut catalog,
            0,
            "track",
            "7",
            false,
            "{track:02} - {title}",
            "opus",
        ));
        assert_eq!(catalog.titles[0].output_file, "07 - Track 1.opus");

        catalog.titles[0].output_file = "My custom name.opus".to_owned();
        assert!(!set_tag(
            &mut catalog,
            0,
            "track",
            "8",
            false,
            "{track:02} - {title}",
            "opus",
        ));
        assert_eq!(catalog.titles[0].output_file, "My custom name.opus");

        let album_art = CoverArt::new("image/png", vec![1]);
        set_cover_art(&mut catalog, None, album_art.clone());
        set_cover_art(&mut catalog, Some(0), album_art);
        assert!(!catalog.track_cover_art.contains_key(&0));

        set_cover_art(&mut catalog, Some(0), CoverArt::new("image/jpeg", vec![2]));
        assert!(matches!(
            catalog.track_cover_art.get(&0),
            Some(TrackCoverArt::Custom(_))
        ));
        remove_cover_art(&mut catalog, Some(0));
        assert_eq!(
            catalog.track_cover_art.get(&0),
            Some(&TrackCoverArt::Removed)
        );
    }

    #[test]
    fn cover_art_type_is_detected_from_file_contents() {
        assert_eq!(
            cover_art_mime_type(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
            Some("image/png")
        );
        assert_eq!(
            cover_art_mime_type(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(cover_art_mime_type(b"not an image"), None);
    }
}
