use super::*;
use media_rip_core::{
    AudioMetadataCandidate, Chapter, MediaCatalog, MediaSource, MediaTag, MetadataAttribution,
    StreamKind, Title, Track, VideoMetadata, VideoMetadataSuggestion,
};

fn catalog(kind: DiscKind) -> MediaCatalog {
    MediaCatalog {
        source: MediaSource {
            id: "test-source".into(),
            name: "Test album".into(),
            path: PathBuf::from("/tmp/test-media"),
            kind,
            drive: None,
            capacity_bytes: None,
            encrypted: None,
        },
        titles: vec![Title {
            dvd: None,
            id: 0,
            source_title_id: "1".into(),
            disc_title_number: Some(1),
            name: "Original title".into(),
            is_main_title: true,
            duration_ms: 2_400_000,
            size_bytes: None,
            size_is_estimate: false,
            chapters: vec![
                Chapter {
                    index: 0,
                    start_ms: 0,
                    end_ms: 1_200_000,
                    name: None,
                },
                Chapter {
                    index: 1,
                    start_ms: 1_200_000,
                    end_ms: 2_400_000,
                    name: None,
                },
            ],
            tracks: vec![Track {
                index: 0,
                kind: if kind == DiscKind::AudioCd {
                    StreamKind::Audio
                } else {
                    StreamKind::Video
                },
                codec: "pcm_s16le".into(),
                codec_long_name: None,
                language: None,
                name: None,
                properties: vec![],
                selected: true,
            }],
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: vec![],
            video_metadata: None,
            output_file: "original.mkv".into(),
            selected: true,
        }],
        source_info: vec![],
        cover_art: None,
        track_cover_art: Default::default(),
        metadata_attributions: vec![],
        audio_metadata_candidates: vec![],
        video_metadata_suggestions: vec![],
        video_episode_context: None,
    }
}

fn video_metadata(content_type: &str) -> VideoMetadata {
    VideoMetadata {
        attribution: MetadataAttribution::new("Test", 90, "Fixture"),
        content_type: Some(content_type.into()),
        year: Some(2020),
        season: None,
        episode: None,
        tmdb_id: None,
        tvdb_id: None,
        tvmaze_id: None,
    }
}

#[test]
fn metadata_context_uses_identity_and_observed_streams() {
    use media_rip_core::model::metadata::MetadataKind;
    let mut source = catalog(DiscKind::Dvd);
    source.titles[0].tracks.clear();
    assert_eq!(
        MetadataKind::for_title(&source, &source.titles[0]),
        MetadataKind::Video
    );
    source.video_episode_context = Some(media_rip_core::VideoEpisodeContext {
        metadata: video_metadata("Series"),
        choices: vec![],
    });
    assert_eq!(
        MetadataKind::for_title(&source, &source.titles[0]),
        MetadataKind::Television
    );
    source.titles[0].video_metadata = Some(video_metadata("MainMovie"));
    assert_eq!(
        MetadataKind::for_title(&source, &source.titles[0]),
        MetadataKind::Film
    );
    let mut cd = catalog(DiscKind::AudioCd);
    assert_eq!(
        MetadataKind::for_title(&cd, &cd.titles[0]),
        MetadataKind::Music
    );
    cd.titles[0]
        .tags
        .push(MediaTag::new("releasetype", "album; audiobook"));
    assert_eq!(
        MetadataKind::for_title(&cd, &cd.titles[0]),
        MetadataKind::Audiobook
    );
    cd.titles[0].tags.push(MediaTag::new("media_type", "music"));
    assert_eq!(
        MetadataKind::for_title(&cd, &cd.titles[0]),
        MetadataKind::Music
    );
}

#[test]
fn replacing_episode_with_film_removes_stale_identity_and_keeps_custom_tags() {
    let mut source = catalog(DiscKind::Dvd);
    source.titles[0].tags = vec![
        MediaTag::new("SHOW", "Old series"),
        MediaTag::new("season_number", "1"),
        MediaTag::new("episode_id", "3"),
        MediaTag::new("tvdb_id", "42"),
        MediaTag::new("comment", "My notes"),
    ];
    source
        .video_metadata_suggestions
        .push(VideoMetadataSuggestion {
            title_id: 0,
            name: "Film".into(),
            tags: vec![MediaTag::new("title", "Film")],
            metadata: video_metadata("MainMovie"),
            evidence: vec![],
            assumptions: vec![],
            episode_choices: vec![],
        });
    assert!(media_rip_core::model::editing::apply_video_suggestion(
        &mut source,
        0,
        "mkv"
    ));
    assert_eq!(
        source.titles[0].tags,
        vec![
            MediaTag::new("comment", "My notes"),
            MediaTag::new("title", "Film")
        ]
    );
    set_tag(&mut source, 0, "date", "2024-03-12", false, "", "mkv");
    assert_eq!(
        source.titles[0].video_metadata.as_ref().unwrap().year,
        Some(2024)
    );
}

#[test]
fn episode_corrections_preserve_film_suggestions_on_mixed_sources() {
    use media_rip_core::model::editing::{
        replace_suggestion_season_choices, shift_video_suggestions_with_result,
    };
    let mut source = catalog(DiscKind::Dvd);
    let film = VideoMetadataSuggestion {
        title_id: 0,
        name: "Film".into(),
        tags: vec![],
        metadata: video_metadata("MainMovie"),
        evidence: vec![],
        assumptions: vec![],
        episode_choices: vec![],
    };
    let choices = vec![media_rip_core::VideoEpisodeChoice {
        episode: 2,
        name: "Second episode".into(),
        tags: vec![],
    }];
    let mut episode = film.clone();
    episode.title_id = 1;
    episode.metadata = video_metadata("Episode");
    episode.metadata.season = Some(1);
    episode.metadata.episode = Some(1);
    episode.episode_choices = choices.clone();
    source.video_metadata_suggestions = vec![film.clone(), episode];
    let shift = shift_video_suggestions_with_result(&mut source, 1)
        .unwrap()
        .unwrap();
    assert_eq!(shift.unmatched, 0);
    assert_eq!(source.video_metadata_suggestions[0], film);
    assert_eq!(
        source.video_metadata_suggestions[1].metadata.episode,
        Some(2)
    );
    assert_eq!(
        replace_suggestion_season_choices(&mut source, 2, &choices),
        Some(1)
    );
    assert_eq!(source.video_metadata_suggestions[0], film);
    assert_eq!(
        source.video_metadata_suggestions[1].metadata.season,
        Some(2)
    );
}

#[test]
fn audio_files_use_audio_defaults_and_ask_mode_ignores_folder_overrides() {
    let mut settings = AppSettings::default();
    settings.audio_cd.output_format = "opus".to_owned();
    settings.output.video_profile = "h265-mkv".to_owned();
    let mut audio = catalog(DiscKind::File);
    audio.titles[0].tracks[0].kind = StreamKind::Audio;
    assert_eq!(
        settings.profile_for_catalog(&audio),
        OutputProfile::AudioOpus
    );
    assert_eq!(
        settings.profile_for_catalog(&catalog(DiscKind::Dvd)),
        OutputProfile::H265Mkv
    );
    settings.output.destination_mode = "none".to_owned();
    settings.output.dvd_directory = "/tmp/films".to_owned();
    assert!(
        default_output_directory("Film", DiscKind::Dvd, false, &settings)
            .as_os_str()
            .is_empty()
    );
}

#[test]
fn automatic_audio_save_requires_real_names_instead_of_a_provider_label() {
    let mut cd = catalog(DiscKind::AudioCd);
    cd.source.name = "Audio CD".to_owned();
    cd.source_info
        .push(media_rip_core::InfoField::new("Tag source", "CD-TEXT"));
    assert!(!audio_cd_metadata_available(&cd));
    cd.titles[0].tags = vec![
        MediaTag::new("album", "A book"),
        MediaTag::new("title", "Track 01"),
    ];
    assert!(!audio_cd_metadata_available(&cd));
    cd.titles[0].tags[1].value = "Chapter One".to_owned();
    assert!(audio_cd_metadata_available(&cd));
}

fn dvd_review_catalog() -> MediaCatalog {
    let mut source = catalog(DiscKind::Dvd);
    source.titles[0].id = 65;
    source.titles[0].source_title_id = "65".to_owned();
    source.titles[0].disc_title_number = Some(65);
    source.titles[0].duplicate_key = Some("dvd-cell-sequence".to_owned());
    source.titles[0].dvd = Some(media_rip_core::model::dvd::DvdTitleInfo {
        title_set: 1,
        title_in_set: 1,
        chapter_range: Some(media_rip_core::model::dvd::DvdChapterRange { first: 9, last: 24 }),
        cell_count: 16,
        leading_padding_cells: 8,
        trailing_padding_cells: 1,
        warnings: vec![],
    });
    let mut duplicate = source.titles[0].clone();
    duplicate.id = 66;
    duplicate.source_title_id = "66".to_owned();
    duplicate.disc_title_number = Some(66);
    duplicate.is_main_title = false;
    let mut suspect = duplicate.clone();
    suspect.id = 67;
    suspect.source_title_id = "67".to_owned();
    suspect.duplicate_key = None;
    suspect
        .dvd
        .as_mut()
        .unwrap()
        .warnings
        .push("Cells jump backwards".to_owned());
    source.titles.extend([duplicate, suspect]);
    source
}

#[test]
fn dvd_defaults_only_select_unique_structurally_checked_paths() {
    let mut source = dvd_review_catalog();
    AppSettings::default().apply_catalog_defaults(&mut source);
    assert_eq!(
        source
            .titles
            .iter()
            .filter(|title| title.selected)
            .map(|title| title.id)
            .collect::<Vec<_>>(),
        [65]
    );
    let json = serde_json::to_string(&source).unwrap();
    assert_eq!(serde_json::from_str::<MediaCatalog>(&json).unwrap(), source);
}

fn widgets<W: IsA<gtk::Widget> + Clone + 'static>(root: &impl IsA<gtk::Widget>) -> Vec<W> {
    let root = root.as_ref();
    let mut found = root
        .clone()
        .downcast::<W>()
        .ok()
        .into_iter()
        .collect::<Vec<_>>();
    let mut child = root.first_child();
    while let Some(widget) = child {
        found.extend(widgets::<W>(&widget));
        child = widget.next_sibling();
    }
    found
}

fn metadata_has_label(controller: &Controller, text: &str) -> bool {
    widgets::<gtk::Label>(&controller.ui.metadata.root)
        .iter()
        .any(|label| label.text().contains(text))
}

#[test]
#[ignore = "requires a GTK display; run with xvfb-run -a cargo test -p media-rip gtk_workflows -- --ignored"]
fn gtk_workflows() {
    gtk::init().unwrap();
    crate::ui::install_styles();
    let application = gtk::Application::builder()
        .application_id("io.github.mediarip.WorkflowTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    application.register(None::<&gio::Cancellable>).unwrap();
    let (events, _) = mpsc::channel();
    let controller = Controller {
        ui: MainWindow::new(&application),
        state: Rc::new(RefCell::new(AppState::default())),
        engine: Arc::new(OpenMediaEngine::new()),
        events,
        worker_events: Rc::new(RefCell::new(None)),
    };
    controller.bind_controls();
    controller.ui.window.present();
    for kind in [
        DiscKind::Dvd,
        DiscKind::BluRay,
        DiscKind::AudioCd,
        DiscKind::File,
    ] {
        controller.state.borrow_mut().busy = BusyState::Scanning;
        controller.handle_event(EngineEvent::ScanFinished(Box::new(catalog(kind))));
        assert!(!controller.state.borrow().is_busy());
        assert!(controller.state.borrow().metadata_step_complete);
        controller.ui.workflow_metadata_button.emit_clicked();
        controller.handle_info_action(InfoAction::SetTitleName {
            title_id: 0,
            name: "Manually corrected".into(),
        });
        let state = controller.state.borrow();
        let title = &state.catalog.as_ref().unwrap().titles[0];
        assert_eq!(title.name, "Manually corrected");
        assert!(
            title
                .tags
                .contains(&MediaTag::new("title", "Manually corrected"))
        );
    }

    let mut film = catalog(DiscKind::Dvd);
    film.titles[0].video_metadata = Some(video_metadata("MainMovie"));
    controller.handle_event(EngineEvent::ScanFinished(Box::new(film)));
    controller.ui.workflow_metadata_button.emit_clicked();
    assert!(metadata_has_label(&controller, "Film metadata"));
    assert!(!metadata_has_label(&controller, "Episode assignments"));
    assert!(!metadata_has_label(&controller, "No episode metadata"));
    let title_entry = widgets::<gtk::Entry>(&controller.ui.metadata.root)
        .into_iter()
        .find(|entry| entry.text() == "Original title")
        .unwrap();
    title_entry.set_text("Edited film");
    assert_eq!(
        controller.state.borrow().catalog.as_ref().unwrap().titles[0].name,
        "Edited film"
    );
    controller.handle_event(EngineEvent::ScanFinished(Box::new(catalog(
        DiscKind::AudioCd,
    ))));
    controller.ui.workflow_metadata_button.emit_clicked();
    assert!(metadata_has_label(&controller, "Album artist"));
    assert!(!metadata_has_label(&controller, "Narrator"));
    widgets::<gtk::DropDown>(&controller.ui.metadata.root)[0].set_selected(1);
    assert!(metadata_has_label(&controller, "Narrator"));
    assert!(metadata_has_label(&controller, "Author"));
    assert!(!metadata_has_label(&controller, "Album artist"));
    controller.state.borrow_mut().busy = BusyState::Ripping;
    controller.render();
    assert!(!controller.ui.metadata.root.is_sensitive());
    controller.handle_event(EngineEvent::RipCancelled);

    let mut cd = catalog(DiscKind::AudioCd);
    cd.audio_metadata_candidates.push(AudioMetadataCandidate {
        id: "edition:1".into(),
        album: "Book".into(),
        artist: "Narrator".into(),
        edition: "Unabridged · disc 1/8".into(),
        evidence: "Two editions share this disc identifier".into(),
        tracks: [(
            1,
            vec![
                MediaTag::new("title", "Chapter One"),
                MediaTag::new("album", "Book"),
                MediaTag::new("artist", "Narrator"),
            ],
        )]
        .into(),
    });
    controller.handle_event(EngineEvent::ScanFinished(Box::new(cd.clone())));
    assert!(!controller.state.borrow().metadata_step_complete);
    assert!(!controller.ui.workflow_output_button.is_sensitive());
    controller.handle_content_action(ContentAction::ApplyAudioCandidate("edition:1".into()));
    assert!(controller.state.borrow().metadata_step_complete);
    assert_eq!(
        controller.state.borrow().catalog.as_ref().unwrap().titles[0].name,
        "Chapter One"
    );
    assert!(controller.ui.workflow_output_button.is_sensitive());
    controller.handle_event(EngineEvent::ScanFinished(Box::new(cd)));
    controller.handle_content_action(ContentAction::SkipMetadata);
    assert!(controller.state.borrow().metadata_skipped);
    assert_eq!(
        controller.state.borrow().catalog.as_ref().unwrap().titles[0].name,
        "Original title"
    );

    let mut dvd = catalog(DiscKind::Dvd);
    dvd.video_metadata_suggestions
        .push(VideoMetadataSuggestion {
            title_id: 0,
            name: "Series - S01E01 - Pilot".into(),
            tags: vec![MediaTag::new("title", "Pilot")],
            metadata: VideoMetadata {
                attribution: MetadataAttribution::new("Test", 70, "Runtime proposal"),
                content_type: Some("Episode".into()),
                year: None,
                season: Some(1),
                episode: Some(1),
                tmdb_id: None,
                tvdb_id: None,
                tvmaze_id: None,
            },
            evidence: vec!["Compatible runtime".into()],
            assumptions: vec!["Disc order is uncertain".into()],
            episode_choices: vec![],
        });
    controller.handle_event(EngineEvent::ScanFinished(Box::new(dvd)));
    assert!(!controller.state.borrow().metadata_step_complete);
    controller.handle_content_action(ContentAction::ApplySuggestions(vec![0]));
    assert_eq!(
        controller.state.borrow().catalog.as_ref().unwrap().titles[0]
            .video_metadata
            .as_ref()
            .unwrap()
            .episode,
        Some(1)
    );
    controller.state.borrow_mut().busy = BusyState::Ripping;
    controller.handle_info_action(InfoAction::SetTitleName {
        title_id: 0,
        name: "Must not change while ripping".into(),
    });
    assert!(
        controller.state.borrow().catalog.as_ref().unwrap().titles[0]
            .name
            .contains("Pilot")
    );
    controller.handle_event(EngineEvent::RipCancelled);
    assert!(!controller.state.borrow().is_busy());
    controller.state.borrow_mut().busy = BusyState::Scanning;
    controller.handle_event(EngineEvent::ScanFailed(
        "Provider unavailable; retry or edit metadata".into(),
    ));
    assert!(!controller.state.borrow().is_busy());
    controller.handle_event(EngineEvent::ScanFinished(Box::new(dvd_review_catalog())));
    assert!(
        widgets::<gtk::Expander>(&controller.ui.content.stack)
            .iter()
            .any(|expander| expander
                .label()
                .is_some_and(|label| label.contains("2 duplicate or unverified DVD")))
    );
    assert_eq!(
        controller
            .state
            .borrow()
            .catalog
            .as_ref()
            .unwrap()
            .titles
            .iter()
            .filter(|title| title.selected)
            .count(),
        1
    );
    SettingsDialog::exercise_workflow(&controller.ui.window);
    controller.ui.window.close();
}
