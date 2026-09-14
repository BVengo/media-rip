use super::*;

fn descendants<W: IsA<gtk::Widget> + Clone + 'static>(root: &impl IsA<gtk::Widget>) -> Vec<W> {
    let widget = root.as_ref();
    let mut found = widget
        .clone()
        .downcast::<W>()
        .ok()
        .into_iter()
        .collect::<Vec<_>>();
    let mut child = widget.first_child();
    while let Some(widget) = child {
        found.extend(descendants::<W>(&widget));
        child = widget.next_sibling();
    }
    found
}

fn control<W: IsA<gtk::Widget> + Clone + 'static>(dialog: &gtk::Dialog, title: &str) -> W {
    let label = descendants::<gtk::Label>(dialog)
        .into_iter()
        .find(|label| label.text() == title)
        .unwrap_or_else(|| panic!("missing setting {title}"));
    let row = label.parent().unwrap().parent().unwrap();
    descendants::<W>(&row)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("missing control for {title}"))
}

impl SettingsDialog {
    // Called by the existing GTK workflow test, on its sole GTK thread.
    pub(crate) fn exercise_workflow(parent: &gtk::ApplicationWindow) {
        let mut settings = AppSettings::default();
        settings.selection.preferred_languages = vec!["eng".into(), "swe".into()];
        settings.integrations.tmdb_access_token = "existing-token".into();
        let Self { dialog, form } = Self::new(parent, &settings);
        dialog.present();
        capture_pages(&dialog);
        assert_eq!(form.settings(), settings);
        assert!(!form.apply.is_sensitive());
        assert!(!form.error.is_visible());
        for removed in [
            "Expert controls",
            "Read buffer size",
            "Discovery mode",
            "Enable streaming server",
            "FFmpeg executable",
        ] {
            assert!(
                !descendants::<gtk::Label>(&dialog)
                    .iter()
                    .any(|label| label.text() == removed)
            );
        }
        let mode = control::<gtk::ComboBoxText>(&dialog, "Default destination");
        mode.set_active_id(Some("custom"));
        assert!(form.error.is_visible());
        assert!(!form.apply.is_sensitive());
        control::<gtk::Entry>(&dialog, "Chosen folder").set_text("/tmp/archive");
        assert!(form.apply.is_sensitive());
        assert!(form.close.label().unwrap().contains("Discard"));
        mode.set_active_id(Some("none"));
        assert!(!control::<gtk::Entry>(&dialog, "DVD folder").is_sensitive());
        assert!(!control::<gtk::Entry>(&dialog, "Chosen folder").is_sensitive());
        mode.set_active_id(Some("automatic"));
        assert!(control::<gtk::Entry>(&dialog, "DVD folder").is_sensitive());
        control::<gtk::ComboBoxText>(&dialog, "Subtitle tracks").set_active_id(Some("none"));
        assert!(!control::<gtk::Switch>(&dialog, "Also include forced subtitles").is_sensitive());
        control::<gtk::Switch>(&dialog, "Allow online metadata lookups").set_active(false);
        assert!(!control::<gtk::PasswordEntry>(&dialog, "TMDB read access token").is_sensitive());
        assert!(control::<gtk::Switch>(&dialog, "Use CD-TEXT").is_sensitive());
        assert_eq!(
            form.settings().integrations.tmdb_access_token,
            "existing-token"
        );
        control::<gtk::Switch>(&dialog, "Send a completion notification").set_active(true);
        assert!(!form.apply.is_sensitive());
        control::<gtk::Entry>(&dialog, "Topic").set_text("test-topic");
        assert!(form.apply.is_sensitive());
        assert_eq!(
            form.settings().selection.preferred_languages,
            ["eng", "swe"]
        );
        let search = descendants::<gtk::SearchEntry>(&dialog).remove(0);
        search.set_text("MusicBrainz");
        search.emit_by_name::<()>("search-changed", &[]);
        let stack = descendants::<gtk::Stack>(&dialog)
            .into_iter()
            .find(|stack| stack.child_by_name("metadata").is_some())
            .unwrap();
        assert_eq!(stack.visible_child_name().as_deref(), Some("metadata"));
        search.set_text("no-such-setting");
        search.emit_by_name::<()>("search-changed", &[]);
        assert_eq!(stack.visible_child_name().as_deref(), Some("empty"));
        search.set_text("");
        search.emit_by_name::<()>("search-changed", &[]);
        assert_ne!(stack.visible_child_name().as_deref(), Some("empty"));
        form.mark_applied();
        assert!(!form.apply.is_sensitive());
        assert_eq!(form.close.label().as_deref(), Some("Close"));
        assert_eq!(settings.output.destination_mode, "automatic");
        assert!(!settings.notifications.ntfy_enabled);
        dialog.close();
    }
}

// Optional artefacts for checking the real GTK layout, not snapshot assertions.
fn capture_pages(dialog: &gtk::Dialog) {
    let Some(directory) = std::env::var_os("MEDIA_RIP_SETTINGS_SCREENSHOTS") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    for (title, filename) in [
        ("Output", "output"),
        ("Selection", "selection"),
        ("Metadata", "metadata"),
        ("Disc automation", "automation"),
        ("Completion", "completion"),
    ] {
        descendants::<gtk::ToggleButton>(dialog)
            .into_iter()
            .find(|button| button.label().as_deref() == Some(title))
            .unwrap()
            .set_active(true);
        let main_loop = gtk::glib::MainLoop::new(None, false);
        let stop = main_loop.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            stop.quit()
        });
        main_loop.run();
        let paintable = gtk::WidgetPaintable::new(Some(dialog));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(
            &snapshot,
            f64::from(dialog.width()),
            f64::from(dialog.height()),
        );
        let node = snapshot.to_node().expect("settings dialog renders a node");
        dialog
            .renderer()
            .unwrap()
            .render_texture(&node, None)
            .save_to_png(directory.join(format!("{filename}.png")))
            .unwrap();
    }
    descendants::<gtk::ToggleButton>(dialog)
        .into_iter()
        .find(|button| button.label().as_deref() == Some("Output"))
        .unwrap()
        .set_active(true);
}
