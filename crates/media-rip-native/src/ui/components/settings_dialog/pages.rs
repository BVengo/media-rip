use super::{controls::Page, *};

pub(super) fn build(
    form: &Rc<SettingsForm>,
    dialog: &gtk::Dialog,
    settings: &AppSettings,
) -> Vec<Page> {
    vec![
        output(form, dialog, settings),
        selection(form, settings),
        metadata(form, settings),
        automation(form, settings),
        completion(form, settings),
    ]
}

fn output(form: &Rc<SettingsForm>, dialog: &gtk::Dialog, s: &AppSettings) -> Page {
    let mut page = Page::new(
        "output",
        "Output",
        "Destinations and output formats for every media type.",
    );
    let folders = page.section("Output folders", "Ask every time leaves the destination blank. Otherwise a disc-specific folder overrides the default.");
    folders.row("Default destination", "Automatic creates a source-named folder under Music or Videos when available, otherwise the working folder.",
        &form.choice(&s.output.destination_mode, &[("none", "Ask every time"), ("automatic", "Automatic source folder"), ("custom", "Use a chosen folder")], |s, value| s.output.destination_mode = value));
    let custom = folders.row(
        "Chosen folder",
        "Used for sources without a disc-specific folder override.",
        &form.folder(dialog, &s.output.custom_directory, |s, value| {
            s.output.custom_directory = value
        }),
    );
    form.enable_when(&custom, |s| s.output.destination_mode == "custom");
    for (label, value, set) in [
        (
            "Audio CD folder",
            s.output.audio_cd_directory.as_str(),
            (|s: &mut AppSettings, value| s.output.audio_cd_directory = value)
                as fn(&mut AppSettings, String),
        ),
        (
            "DVD folder",
            s.output.dvd_directory.as_str(),
            (|s: &mut AppSettings, value| s.output.dvd_directory = value)
                as fn(&mut AppSettings, String),
        ),
        (
            "Blu-ray folder",
            s.output.bluray_directory.as_str(),
            (|s: &mut AppSettings, value| s.output.bluray_directory = value)
                as fn(&mut AppSettings, String),
        ),
    ] {
        let row = folders.row(
            label,
            "Optional. Leave empty to use the default destination.",
            &form.folder(dialog, value, set),
        );
        form.enable_when(&row, |s| s.output.destination_mode != "none");
    }
    let formats = page.section("Output formats", "Defaults for new sources. The Output step can choose a different format for the current job.");
    formats.row(
        "Video format",
        "DVDs, Blu-rays and video files. Preserve original tracks avoids re-encoding.",
        &form.choice(
            &s.output.video_profile,
            &[
                ("copy-mkv", "MKV: preserve original tracks"),
                ("h264-mp4", "MP4: H.264 + AAC"),
                ("h265-mkv", "MKV: H.265"),
                ("av1-webm", "WebM: AV1 + Opus"),
            ],
            |s, value| s.output.video_profile = value,
        ),
    );
    formats.row(
        "Audio format",
        "Music, audiobooks and audio files. FLAC is lossless; WAV has limited tag support.",
        &form.choice(
            &s.audio_cd.output_format,
            &[
                ("flac", "FLAC: lossless"),
                ("wav", "WAV: uncompressed"),
                ("opus", "Opus: compact"),
                ("aac", "M4A: AAC"),
            ],
            |s, value| s.audio_cd.output_format = value,
        ),
    );
    let movies = page.section("Movie filenames", "Applied when movie metadata is confirmed. The selected video format determines the extension.");
    movies.row("Movie path template", "Fields: {movie name}, {movie year}. Use {if:movie year} ... {end} (or if:movie name) to include a group only when that field is present. Groups may nest. Put optional punctuation inside the group. Use / for folders. Leave empty for: {movie name}{if:movie year} ({movie year}){end}.",
        &form.entry(&s.output.movie_file_name_template, |s, value| s.output.movie_file_name_template = value));
    let naming = page.section("Audio CD filenames", "The selected audio format determines the file extension. Metadata characters cannot create extra folders.");
    naming.row("Track path template", "Fields: {artist}, {album}, {disc}, {track}, {track:02}, {title}. Use / to create folders. Leave empty for the default.",
        &form.entry(&s.audio_cd.file_name_template, |s, value| s.audio_cd.file_name_template = value));
    page
}

fn selection(form: &Rc<SettingsForm>, s: &AppSettings) -> Page {
    let mut page = Page::new(
        "selection",
        "Selection",
        "What starts checked when a source is opened. All discovered titles remain visible.",
    );
    let titles = page.section("Video titles", "Applies to DVDs, Blu-rays and video files. Audio CD tracks are never filtered by duration.");
    titles.row("Minimum title length (seconds)", "Shorter titles start unchecked when there is more than one title. Set 0 to include all durations.",
        &form.number(s.output.minimum_title_seconds, |s, value| s.output.minimum_title_seconds = value));
    let streams = page.section("Audio and subtitles", "These defaults select streams inside video titles. Unknown-language streams are kept with preferred-language policies.");
    streams.row(
        "Preferred languages",
        "Used by the preferred-language policies below. No preference selects all languages.",
        &languages(form, &s.selection.preferred_languages),
    );
    streams.row("Audio tracks", "Default means the track marked as default by the source; it may not be the original language.",
        &form.choice(&s.selection.audio_policy, &[("all", "All audio tracks"), ("preferred", "Preferred languages"), ("original-preferred", "Default + preferred languages")], |s, value| s.selection.audio_policy = value));
    streams.row(
        "Subtitle tracks",
        "No subtitles leaves every subtitle unchecked, including forced tracks.",
        &form.choice(
            &s.selection.subtitle_policy,
            &[
                ("all", "All subtitles"),
                ("preferred", "Preferred languages"),
                ("forced", "Forced subtitles only"),
                ("none", "No subtitles"),
            ],
            |s, value| s.selection.subtitle_policy = value,
        ),
    );
    let forced = streams.row(
        "Also include forced subtitles",
        "Add identified forced subtitles in other languages when using preferred languages.",
        &form.toggle(s.selection.include_forced_subtitles, |s, value| {
            s.selection.include_forced_subtitles = value
        }),
    );
    form.enable_when(&forced, |s| s.selection.subtitle_policy == "preferred");
    let television = page.section(
        "Television discs",
        "Used when multiple individual episodes are identified on a DVD or Blu-ray.",
    );
    television.row(
        "Select episodes",
        "Select the individual episode titles.",
        &form.toggle(s.selection.select_tv_episodes, |s, value| {
            s.selection.select_tv_episodes = value
        }),
    );
    television.row("Select play-all titles", "Also select combined titles containing multiple episodes. This can duplicate episode content in the output.", &form.toggle(s.selection.select_tv_play_all, |s, value| s.selection.select_tv_play_all = value));
    television.row(
        "Select extras and unmatched titles",
        "Include non-episode content alongside identified episodes.",
        &form.toggle(s.selection.select_tv_non_episodic, |s, value| {
            s.selection.select_tv_non_episodic = value
        }),
    );
    let main_title = page.section(
        "DVD and Blu-ray main title",
        "Choose the default selection for movie discs.",
    );
    main_title.row(
        "Select only the main title",
        "Use authored Blu-ray flags or a clearly dominant DVD feature. Otherwise use the other selection defaults.",
        &form.toggle(s.bluray.select_only_main_title, |s, value| {
            s.bluray.select_only_main_title = value
        }),
    );
    let bluray = page.section("Blu-ray playlists", "These options affect Blu-rays only.");
    bluray.row("Select exact duplicate playlists", "Include playlists with identical clip ranges. Otherwise only one representative starts checked.", &form.toggle(s.bluray.select_duplicate_playlists, |s, value| s.bluray.select_duplicate_playlists = value));
    page
}

fn metadata(form: &Rc<SettingsForm>, s: &AppSettings) -> Page {
    let mut page = Page::new(
        "metadata",
        "Metadata",
        "Identification and tagging while opening sources. Review and edit individual matches in the Metadata step.",
    );
    let access = page.section(
        "Online lookups",
        "This controls metadata providers. Completion notifications have their own setting.",
    );
    access.row(
        "Allow online metadata lookups",
        "Turn off to avoid metadata requests. Embedded disc metadata remains available.",
        &form.toggle(s.general.allow_online_access, |s, value| {
            s.general.allow_online_access = value
        }),
    );
    let audio = page.section("Audio CDs: music and audiobooks", "Names and release information are matched to the physical disc. Ambiguous releases require a choice.");
    audio.row("Use CD-TEXT", "Use album, artist and track names embedded on the disc when online metadata is unavailable.", &form.toggle(s.audio_cd.use_disc_metadata, |s, value| s.audio_cd.use_disc_metadata = value));
    let musicbrainz = audio.row(
        "Look up releases with MusicBrainz",
        "Find album or audiobook release tags from the disc's track layout.",
        &form.toggle(s.audio_cd.auto_tagging, |s, value| {
            s.audio_cd.auto_tagging = value
        }),
    );
    form.enable_when(&musicbrainz, |s| s.general.allow_online_access);
    let video = page.section("DVD and Blu-ray identification", "Providers complement each other. Credentials are optional and stay in your local settings file.");
    let discdb = video.row(
        "TheDiscDB",
        "Match authored discs using content fingerprints.",
        &form.toggle(s.integrations.thediscdb_enabled, |s, value| {
            s.integrations.thediscdb_enabled = value
        }),
    );
    form.enable_when(&discdb, |s| s.general.allow_online_access);
    let previous = video.row("Continue matching a DVD series", "Reuse the previous DVD's identity and episode position only when the new disc is compatible with that series.", &form.toggle(s.integrations.use_previous_dvd_match, |s, value| s.integrations.use_previous_dvd_match = value));
    form.enable_when(&previous, |s| s.general.allow_online_access);
    let tvmaze = video.row(
        "TVMaze",
        "TV series, episode runtimes and artwork. No credentials required.",
        &form.toggle(s.integrations.tvmaze_enabled, |s, value| {
            s.integrations.tvmaze_enabled = value
        }),
    );
    form.enable_when(&tvmaze, |s| s.general.allow_online_access);
    let tmdb = video.row(
        "TMDB read access token",
        "Optional movie and television identification and artwork. Leave empty to disable TMDB.",
        &form.password(&s.integrations.tmdb_access_token, |s, value| {
            s.integrations.tmdb_access_token = value
        }),
    );
    form.enable_when(&tmdb, |s| s.general.allow_online_access);
    let tvdb = video.row(
        "TVDB API key",
        "Optional television identification and DVD episode ordering. Leave empty to disable TVDB.",
        &form.password(&s.integrations.tvdb_api_key, |s, value| {
            s.integrations.tvdb_api_key = value
        }),
    );
    form.enable_when(&tvdb, |s| s.general.allow_online_access);
    let pin = video.row(
        "TVDB subscriber PIN",
        "Only needed for a TVDB key using subscriber-supported access.",
        &form.password(&s.integrations.tvdb_subscriber_pin, |s, value| {
            s.integrations.tvdb_subscriber_pin = value
        }),
    );
    form.enable_when(&pin, |s| {
        s.general.allow_online_access && !s.integrations.tvdb_api_key.trim().is_empty()
    });
    page
}

fn automation(form: &Rc<SettingsForm>, s: &AppSettings) -> Page {
    let mut page = Page::new(
        "automation",
        "Disc automation",
        "Automatic reading starts after you manually open the first disc in this session, and only while the application is idle.",
    );
    let audio = page.section(
        "Audio CDs",
        "Secure extraction uses cdparanoia. If saving pauses for review, finish the review and start the job from Output.",
    );
    audio.row(
        "Read Audio CDs automatically",
        "Scan the next disc when the drive reports it is ready.",
        &form.toggle(s.automation.audio_cd.read_automatically, |s, value| {
            s.automation.audio_cd.read_automatically = value
        }),
    );
    audio.row(
        "Save Audio CDs automatically",
        "Save selected tracks after scanning. Pending metadata choices pause automatic saving.",
        &form.toggle(s.automation.audio_cd.save_automatically, |s, value| {
            s.automation.audio_cd.save_automatically = value
        }),
    );
    let required = audio.row(
        "Require album and track metadata",
        "Pause automatic saving when only generic disc or track names are available.",
        &form.toggle(s.automation.require_audio_cd_metadata, |s, value| {
            s.automation.require_audio_cd_metadata = value
        }),
    );
    form.enable_when(&required, |s| s.automation.audio_cd.save_automatically);
    let dvd = page.section(
        "DVDs",
        "Uses the title and stream defaults on the Selection page.",
    );
    dvd.row(
        "Read DVDs automatically",
        "Scan the next DVD when it becomes ready.",
        &form.toggle(s.automation.dvd.read_automatically, |s, value| {
            s.automation.dvd.read_automatically = value
        }),
    );
    dvd.row(
        "Save DVDs automatically",
        "Save selected titles after scanning. Pending metadata choices pause automatic saving.",
        &form.toggle(s.automation.dvd.save_automatically, |s, value| {
            s.automation.dvd.save_automatically = value
        }),
    );
    let bluray = page.section(
        "Blu-rays",
        "Uses the title, stream and playlist defaults on the Selection page.",
    );
    bluray.row(
        "Read Blu-rays automatically",
        "Scan the next Blu-ray when it becomes ready.",
        &form.toggle(s.automation.bluray.read_automatically, |s, value| {
            s.automation.bluray.read_automatically = value
        }),
    );
    bluray.row(
        "Save Blu-rays automatically",
        "Save selected playlists after scanning. Pending metadata choices pause automatic saving.",
        &form.toggle(s.automation.bluray.save_automatically, |s, value| {
            s.automation.bluray.save_automatically = value
        }),
    );
    page
}

fn completion(form: &Rc<SettingsForm>, s: &AppSettings) -> Page {
    let mut page = Page::new(
        "completion",
        "Completion",
        "Actions after every selected item in a job has been saved successfully.",
    );
    let local = page.section(
        "On this computer",
        "These actions do not run after a failed or cancelled job.",
    );
    local.row(
        "Eject the disc",
        "Eject the source drive when the whole job succeeds.",
        &form.toggle(s.io.eject_when_finished, |s, value| {
            s.io.eject_when_finished = value
        }),
    );
    local.row(
        "Play a completion sound",
        "Use the desktop's standard alert sound.",
        &form.toggle(s.notifications.completion_sound_enabled, |s, value| {
            s.notifications.completion_sound_enabled = value
        }),
    );
    let ntfy = page.section(
        "ntfy notifications",
        "Subscribe to the same topic in the ntfy app to receive completion messages.",
    );
    ntfy.row(
        "Send a completion notification",
        "Send the job result to your ntfy server. This is independent of online metadata access.",
        &form.toggle(s.notifications.ntfy_enabled, |s, value| {
            s.notifications.ntfy_enabled = value
        }),
    );
    let server = ntfy.row(
        "Server URL",
        "Use https://ntfy.sh or the HTTP(S) address of your own server.",
        &form.entry(&s.notifications.ntfy_server_url, |s, value| {
            s.notifications.ntfy_server_url = value
        }),
    );
    let topic = ntfy.row(
        "Topic",
        "Choose an unguessable name on a public server. Do not include slashes.",
        &form.entry(&s.notifications.ntfy_topic, |s, value| {
            s.notifications.ntfy_topic = value
        }),
    );
    let token = ntfy.row(
        "Access token",
        "Optional bearer token for a protected topic.",
        &form.password(&s.notifications.ntfy_access_token, |s, value| {
            s.notifications.ntfy_access_token = value
        }),
    );
    for row in [server, topic, token] {
        form.enable_when(&row, |s| s.notifications.ntfy_enabled);
    }
    page
}

const LANGUAGES: &[(&str, &str)] = &[
    ("eng", "English"),
    ("deu", "German"),
    ("spa", "Spanish"),
    ("fra", "French"),
    ("ita", "Italian"),
    ("jpn", "Japanese"),
    ("kor", "Korean"),
    ("por", "Portuguese"),
    ("zho", "Chinese"),
    ("nld", "Dutch"),
];

fn languages(form: &Rc<SettingsForm>, selected: &[String]) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let grid = gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(4)
        .build();
    for (index, &(id, name)) in LANGUAGES.iter().enumerate() {
        let button = gtk::CheckButton::with_label(name);
        button.set_active(selected.iter().any(|value| value == id));
        let form = Rc::downgrade(form);
        button.connect_toggled(move |button| {
            if let Some(form) = form.upgrade() {
                form.update(|s| {
                    s.selection.preferred_languages.retain(|value| value != id);
                    if button.is_active() {
                        s.selection.preferred_languages.push(id.to_owned());
                    }
                });
            }
        });
        grid.attach(&button, (index % 2) as i32, (index / 2) as i32, 1, 1);
    }
    root.append(&grid);
    let additional = selected
        .iter()
        .filter(|value| !LANGUAGES.iter().any(|(id, _)| value == id))
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let entry = form.entry(&additional, |s, value| {
        s.selection
            .preferred_languages
            .retain(|value| LANGUAGES.iter().any(|(id, _)| value == id));
        for code in value.split([',', ';', ' ']).filter(|code| !code.is_empty()) {
            let code = code.to_ascii_lowercase();
            if !s.selection.preferred_languages.contains(&code) {
                s.selection.preferred_languages.push(code);
            }
        }
    });
    entry.set_placeholder_text(Some("Other language codes, e.g. swe, ara"));
    entry.set_tooltip_text(Some(
        "Comma-separated two- or three-letter language codes. Existing preferences are preserved.",
    ));
    root.append(&entry);
    root
}
