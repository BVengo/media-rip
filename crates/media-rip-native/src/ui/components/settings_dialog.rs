use std::{cell::RefCell, rc::Rc};

use gtk::prelude::*;

use crate::{app::AppSettings, ui::translate};

pub(crate) struct SettingsDialog {
    pub dialog: gtk::Dialog,
    pub form: Rc<SettingsForm>,
}

#[derive(Clone)]
pub(crate) struct SettingsForm {
    data_directory: gtk::Entry,
    debug_logging: gtk::Switch,
    expert_controls: gtk::Switch,
    av_sync_messages: gtk::Switch,
    allow_online_access: gtk::Switch,
    destination_mode: gtk::ComboBoxText,
    custom_directory: gtk::Entry,
    audio_cd_directory: gtk::Entry,
    dvd_directory: gtk::Entry,
    bluray_directory: gtk::Entry,
    minimum_title_seconds: gtk::SpinButton,
    minimum_free_space_gib: gtk::SpinButton,
    preferred_languages: Vec<(String, gtk::CheckButton)>,
    audio_policy: gtk::ComboBoxText,
    subtitle_policy: gtk::ComboBoxText,
    include_forced_subtitles: gtk::Switch,
    select_tv_episodes: gtk::Switch,
    select_tv_play_all: gtk::Switch,
    select_tv_non_episodic: gtk::Switch,
    read_retry_count: gtk::SpinButton,
    read_buffer_mib: gtk::ComboBoxText,
    single_drive_mode: gtk::Switch,
    eject_when_finished: gtk::Switch,
    completion_sound_enabled: gtk::Switch,
    ntfy_enabled: gtk::Switch,
    ntfy_server_url: gtk::Entry,
    ntfy_topic: gtk::Entry,
    ntfy_access_token: gtk::PasswordEntry,
    audio_cd_auto_read: gtk::Switch,
    audio_cd_auto_save: gtk::Switch,
    dvd_auto_read: gtk::Switch,
    dvd_auto_save: gtk::Switch,
    bluray_auto_read: gtk::Switch,
    bluray_auto_save: gtk::Switch,
    require_audio_cd_metadata: gtk::Switch,
    dvd_structure_protection_method: gtk::ComboBoxText,
    dvd_processing_mode: gtk::ComboBoxText,
    bluray_select_only_main_title: gtk::Switch,
    bluray_select_duplicate_playlists: gtk::Switch,
    create_protection_dumps: gtk::Switch,
    java_executable: gtk::Entry,
    audio_cd_output_format: gtk::ComboBoxText,
    audio_cd_file_name_template: gtk::Entry,
    audio_cd_secure_extraction: gtk::Switch,
    audio_cd_use_disc_metadata: gtk::Switch,
    audio_cd_auto_tagging: gtk::Switch,
    thediscdb_enabled: gtk::Switch,
    use_previous_dvd_match: gtk::Switch,
    tvmaze_enabled: gtk::Switch,
    tmdb_access_token: gtk::PasswordEntry,
    tvdb_api_key: gtk::PasswordEntry,
    tvdb_subscriber_pin: gtk::PasswordEntry,
    optical_access_provider: gtk::ComboBoxText,
    expose_to_media_players: gtk::Switch,
    streaming_enabled: gtk::Switch,
    streaming_address: gtk::Entry,
    streaming_port: gtk::SpinButton,
    default_profile: gtk::ComboBoxText,
    default_selection_rule: gtk::Entry,
    output_file_name_template: gtk::Entry,
    ffmpeg_executable: gtk::Entry,
    ccextractor_executable: gtk::Entry,
    temporary_directory: gtk::Entry,
    initial_settings: RefCell<AppSettings>,
    apply_button: gtk::Button,
}

impl SettingsDialog {
    pub fn new(parent: &gtk::ApplicationWindow, settings: &AppSettings) -> Self {
        let dialog = gtk::Dialog::builder()
            .title(translate("Settings"))
            .transient_for(parent)
            .modal(true)
            .default_width(980)
            .default_height(700)
            .build();
        dialog.add_css_class("settings-dialog");
        dialog.add_button(&translate("Cancel"), gtk::ResponseType::Cancel);
        dialog.add_button(&translate("Apply"), gtk::ResponseType::Accept);
        crate::ui::space_dialog_actions(
            &dialog,
            &[gtk::ResponseType::Cancel, gtk::ResponseType::Accept],
        );
        dialog.set_default_response(gtk::ResponseType::Accept);
        let apply_button = dialog
            .widget_for_response(gtk::ResponseType::Accept)
            .and_then(|widget| widget.downcast::<gtk::Button>().ok())
            .expect("dialog accept response should be a button");
        apply_button.add_css_class("suggested-action");
        apply_button.set_sensitive(false);

        let (root, form) = SettingsForm::build(&dialog, settings, apply_button);
        dialog.content_area().append(&root);
        form.connect_change_tracking();

        Self { dialog, form }
    }
}

impl SettingsForm {
    fn build(
        dialog: &gtk::Dialog,
        settings: &AppSettings,
        apply_button: gtk::Button,
    ) -> (gtk::Box, Rc<Self>) {
        let data_directory = entry(&settings.general.data_directory, "Use the platform default");
        let debug_logging = switch(settings.general.debug_logging);
        let expert_controls = switch(settings.general.expert_controls);
        let av_sync_messages = switch(settings.general.av_sync_messages);
        let allow_online_access = switch(settings.general.allow_online_access);

        let destination_mode = combo(
            &[
                ("none", "Ask every time"),
                ("automatic", "Automatic"),
                ("semi-automatic", "Automatic (confirmation planned)"),
                ("custom", "Custom folder"),
            ],
            &settings.output.destination_mode,
        );
        let custom_directory = entry(
            &settings.output.custom_directory,
            "Choose a default output folder",
        );
        let audio_cd_directory = entry(
            &settings.output.audio_cd_directory,
            "Use the global default destination",
        );
        let dvd_directory = entry(
            &settings.output.dvd_directory,
            "Use the global default destination",
        );
        let bluray_directory = entry(
            &settings.output.bluray_directory,
            "Use the global default destination",
        );
        let minimum_title_seconds = spin(
            0.0,
            86_400.0,
            1.0,
            settings.output.minimum_title_seconds as f64,
        );
        let minimum_free_space_gib = spin(
            0.0,
            1_024.0,
            1.0,
            settings.output.minimum_free_space_gib as f64,
        );

        let (preferred_language_list, preferred_languages) =
            language_checklist(&settings.selection.preferred_languages);
        let audio_policy = combo(
            &[
                ("all", "All audio tracks"),
                ("preferred", "Preferred language"),
                ("original-preferred", "Original + preferred language"),
            ],
            &settings.selection.audio_policy,
        );
        let subtitle_policy = combo(
            &[
                ("all", "All subtitle tracks"),
                ("preferred", "Preferred language"),
                ("forced", "Forced only"),
                ("none", "No subtitles"),
            ],
            &settings.selection.subtitle_policy,
        );
        let include_forced_subtitles = switch(settings.selection.include_forced_subtitles);
        let select_tv_episodes = switch(settings.selection.select_tv_episodes);
        let select_tv_play_all = switch(settings.selection.select_tv_play_all);
        let select_tv_non_episodic = switch(settings.selection.select_tv_non_episodic);

        let read_retry_count = spin(0.0, 99.0, 1.0, settings.io.read_retry_count as f64);
        let read_buffer_mib = combo(
            &[
                ("0", "Automatic"),
                ("128", "128 MiB"),
                ("256", "256 MiB"),
                ("512", "512 MiB"),
                ("1024", "1 GiB"),
                ("2048", "2 GiB"),
            ],
            &settings.io.read_buffer_mib.to_string(),
        );
        let single_drive_mode = switch(settings.io.single_drive_mode);
        let eject_when_finished = switch(settings.io.eject_when_finished);
        let completion_sound_enabled = switch(settings.notifications.completion_sound_enabled);
        let ntfy_enabled = switch(settings.notifications.ntfy_enabled);
        let ntfy_server_url = entry(
            &settings.notifications.ntfy_server_url,
            "https://ntfy.sh or http://192.168.1.20:8080",
        );
        let ntfy_topic = entry(&settings.notifications.ntfy_topic, "A private topic name");
        let ntfy_access_token = gtk::PasswordEntry::builder()
            .text(&settings.notifications.ntfy_access_token)
            .placeholder_text("Optional access token")
            .show_peek_icon(true)
            .width_request(260)
            .build();
        let audio_cd_auto_read = switch(settings.automation.audio_cd.read_automatically);
        let audio_cd_auto_save = switch(settings.automation.audio_cd.save_automatically);
        let dvd_auto_read = switch(settings.automation.dvd.read_automatically);
        let dvd_auto_save = switch(settings.automation.dvd.save_automatically);
        let bluray_auto_read = switch(settings.automation.bluray.read_automatically);
        let bluray_auto_save = switch(settings.automation.bluray.save_automatically);
        let require_audio_cd_metadata = switch(settings.automation.require_audio_cd_metadata);

        let dvd_structure_protection_method = combo(
            &[
                ("automatic", "Automatic"),
                ("cell-walk", "CellWalk"),
                ("cell-trim", "CellTrim"),
                ("cell-full", "CellFull"),
                ("none", "Do not repair"),
            ],
            &settings.dvd.structure_protection_method,
        );
        let dvd_processing_mode = combo(
            &[
                ("automatic", "Automatic"),
                ("titles", "Titles"),
                ("titlesets", "Title sets"),
            ],
            &settings.dvd.processing_mode,
        );

        let bluray_select_only_main_title = switch(settings.bluray.select_only_main_title);
        let bluray_select_duplicate_playlists = switch(settings.bluray.select_duplicate_playlists);
        let create_protection_dumps = switch(settings.bluray.create_protection_dumps);
        let java_executable = entry(
            &settings.bluray.java_executable,
            "Use Java from the system path",
        );

        let audio_cd_output_format = combo(
            &[
                ("flac", "FLAC — lossless"),
                ("wav", "WAV — uncompressed"),
                ("opus", "Opus — compact"),
                ("aac", "M4A — compatible"),
            ],
            &settings.audio_cd.output_format,
        );
        let audio_cd_file_name_template = entry(
            &settings.audio_cd.file_name_template,
            "{artist}/{album}/{disc}-{track:02} - {title}.flac",
        );
        let audio_cd_secure_extraction = switch(settings.audio_cd.secure_extraction);
        let audio_cd_use_disc_metadata = switch(settings.audio_cd.use_disc_metadata);
        let audio_cd_auto_tagging = switch(settings.audio_cd.auto_tagging);
        audio_cd_secure_extraction.set_active(true);
        audio_cd_secure_extraction.set_sensitive(false);

        let thediscdb_enabled = switch(settings.integrations.thediscdb_enabled);
        let use_previous_dvd_match = switch(settings.integrations.use_previous_dvd_match);
        let tvmaze_enabled = switch(settings.integrations.tvmaze_enabled);
        let tmdb_access_token = gtk::PasswordEntry::builder()
            .text(&settings.integrations.tmdb_access_token)
            .placeholder_text("TMDB API Read Access Token (optional)")
            .show_peek_icon(true)
            .hexpand(true)
            .build();
        tmdb_access_token.set_size_request(328, -1);
        let tvdb_api_key = gtk::PasswordEntry::builder()
            .text(&settings.integrations.tvdb_api_key)
            .placeholder_text("TVDB v4 project API key (optional)")
            .show_peek_icon(true)
            .hexpand(true)
            .build();
        tvdb_api_key.set_size_request(328, -1);
        let tvdb_subscriber_pin = gtk::PasswordEntry::builder()
            .text(&settings.integrations.tvdb_subscriber_pin)
            .placeholder_text("Only needed for user-supported keys")
            .show_peek_icon(true)
            .hexpand(true)
            .build();
        tvdb_subscriber_pin.set_size_request(328, -1);

        let optical_access_provider = combo(
            &[
                ("system", "Automatic (system libraries)"),
                ("none", "Unencrypted media only"),
                ("custom", "Custom provider"),
            ],
            &settings.integrations.optical_access_provider,
        );
        let expose_to_media_players = switch(settings.integrations.expose_to_media_players);
        let streaming_enabled = switch(settings.integrations.streaming_enabled);
        let streaming_address = entry(&settings.integrations.streaming_address, "127.0.0.1");
        let streaming_port = spin(
            1_024.0,
            65_535.0,
            1.0,
            settings.integrations.streaming_port as f64,
        );

        let default_profile = combo(
            &[
                ("copy-mkv", "MKV — preserve original tracks"),
                ("h264-mp4", "MP4 — H.264 + AAC"),
                ("h265-mkv", "MKV — H.265 video"),
                ("av1-webm", "WebM — AV1 + Opus"),
                ("flac-audio", "FLAC — lossless audio"),
                ("wav-audio", "WAV — uncompressed audio"),
                ("opus-audio", "Opus — compact audio"),
                ("aac-audio", "M4A — AAC audio"),
            ],
            &settings.advanced.default_profile,
        );
        let default_selection_rule = entry(
            &settings.advanced.default_selection_rule,
            "Comma-separated selection rules",
        );
        let output_file_name_template = entry(
            &settings.advanced.output_file_name_template,
            "{source}{_t:title:02}",
        );
        let ffmpeg_executable = entry(
            &settings.advanced.ffmpeg_executable,
            "Use ffmpeg from the system path",
        );
        let ccextractor_executable = entry(&settings.advanced.ccextractor_executable, "Optional");
        let temporary_directory = entry(
            &settings.advanced.temporary_directory,
            "Use the output folder",
        );

        let general = page(
            "General",
            "Application behaviour, diagnostics, and network access.",
        );
        let application = section(
            "Application data",
            "Choose where MediaRip keeps logs, metadata caches, and support files.",
        );
        application.append(&path_setting_row(
            "Data directory",
            "Leave empty to use the standard configuration directory.",
            &data_directory,
            dialog,
            "Choose data directory",
            gtk::FileChooserAction::SelectFolder,
        ));
        general.append(&application);

        let diagnostics = section(
            "Diagnostics & access",
            "Extra detail is useful when investigating a difficult disc.",
        );
        diagnostics.append(&setting_row(
            "Log debug messages",
            "Include detailed engine messages in the activity log.",
            &debug_logging,
        ));
        diagnostics.append(&setting_row(
            "Show A/V synchronization messages",
            "Report timestamp corrections found while reading media.",
            &av_sync_messages,
        ));
        diagnostics.append(&setting_row(
            "Expert controls",
            "Expose low-level rules and provider-specific options.",
            &expert_controls,
        ));
        diagnostics.append(&setting_row(
            "Allow online access",
            "Permit metadata lookups and update checks when available.",
            &allow_online_access,
        ));
        general.append(&diagnostics);

        let output = page(
            "Output",
            "Defaults for where rips are saved and which titles are shown.",
        );
        let destination = section(
            "Default destination",
            "Individual jobs can still choose a different folder.",
        );
        destination.append(&setting_row(
            "Destination behaviour",
            "Automatic creates a source-named folder under Videos.",
            &destination_mode,
        ));
        destination.append(&path_setting_row(
            "Custom folder",
            "Global fallback used when a media type has no folder override.",
            &custom_directory,
            dialog,
            "Choose default output folder",
            gtk::FileChooserAction::SelectFolder,
        ));
        output.append(&destination);
        let filtering = section(
            "Title filtering",
            "Keep menus and tiny utility clips out of the initial title list.",
        );
        filtering.append(&setting_row(
            "Minimum title length",
            "Titles shorter than this many seconds remain hidden.",
            &minimum_title_seconds,
        ));
        filtering.append(&setting_row(
            "Minimum free space",
            "Warn before a job starts when fewer GiB are available.",
            &minimum_free_space_gib,
        ));
        output.append(&filtering);

        let selection = page(
            "Audio & subtitles",
            "Preferred-language defaults for audio and subtitle streams.",
        );
        let languages = section(
            "Preferred languages",
            "These choices seed the selection tree when a source is opened.",
        );
        languages.append(&setting_row(
            "Choose languages",
            "Select every language you understand. Leave all unchecked for no preference.",
            &preferred_language_list,
        ));
        selection.append(&languages);
        let tracks = section(
            "Default tracks",
            "Choose which streams are checked when a source is opened.",
        );
        tracks.append(&setting_row(
            "Audio",
            "Lossless and compatibility variants can be refined with an expert rule.",
            &audio_policy,
        ));
        tracks.append(&setting_row(
            "Subtitles",
            "Forced subtitles can be included in addition to this policy.",
            &subtitle_policy,
        ));
        tracks.append(&setting_row(
            "Always include forced subtitles",
            "Select forced tracks when they can be identified.",
            &include_forced_subtitles,
        ));
        tracks.append(&setting_row(
            "TV episodes",
            "Select individual episode titles when a source is identified as a TV show.",
            &select_tv_episodes,
        ));
        tracks.append(&setting_row(
            "TV play-all titles",
            "Select combined play-all titles when matching individual episodes are found.",
            &select_tv_play_all,
        ));
        tracks.append(&setting_row(
            "TV non-episodic content",
            "Select extras and other unmatched titles when individual episodes are found.",
            &select_tv_non_episodic,
        ));
        selection.append(&tracks);

        let io = page(
            "Drives & I/O",
            "Tune optical-drive reads and job completion behaviour.",
        );
        let reading = section(
            "Reading",
            "Defaults are conservative and work well for most drives.",
        );
        reading.append(&setting_row(
            "Read retry count",
            "Additional reads requested after the operating system reports an error.",
            &read_retry_count,
        ));
        reading.append(&setting_row(
            "Read buffer size",
            "Automatic lets the engine choose for the detected media.",
            &read_buffer_mib,
        ));
        reading.append(&setting_row(
            "Single drive mode",
            "Keep one drive active at a time for USB enclosures and constrained systems.",
            &single_drive_mode,
        ));
        io.append(&reading);
        let automation = section(
            "Disc automation",
            "Reading and saving are configured independently for each kind of disc.",
        );
        automation.append(&setting_row(
            "Read Audio CDs automatically",
            "Open an Audio CD as soon as the drive reports that it is ready.",
            &audio_cd_auto_read,
        ));
        automation.append(&setting_row(
            "Save Audio CDs automatically",
            "Start saving all selected tracks immediately after the metadata scan.",
            &audio_cd_auto_save,
        ));
        automation.append(&setting_row(
            "Require Audio CD metadata",
            "Keep the disc open for review instead of auto-saving when only generic track names are available.",
            &require_audio_cd_metadata,
        ));
        automation.append(&setting_row(
            "Read DVDs automatically",
            "Open a DVD when it becomes ready; saving still waits unless enabled separately.",
            &dvd_auto_read,
        ));
        automation.append(&setting_row(
            "Save DVDs automatically",
            "Save the titles selected by the normal filtering rules immediately after reading.",
            &dvd_auto_save,
        ));
        automation.append(&setting_row(
            "Read Blu-rays automatically",
            "Open a Blu-ray when it becomes ready; saving still waits unless enabled separately.",
            &bluray_auto_read,
        ));
        automation.append(&setting_row(
            "Save Blu-rays automatically",
            "Save the playlists selected by the normal filtering rules immediately after reading.",
            &bluray_auto_save,
        ));
        io.append(&automation);
        let completion = section(
            "Completion",
            "What to do after every title is safely saved.",
        );
        completion.append(&setting_row(
            "Eject disc when finished",
            "Only eject after the complete job succeeds.",
            &eject_when_finished,
        ));
        completion.append(&setting_row(
            "Play a completion sound",
            "Play the desktop's standard alert sound after a successful job.",
            &completion_sound_enabled,
        ));
        completion.append(&setting_row(
            "Send an ntfy notification",
            "Notify a subscribed phone after a successful job.",
            &ntfy_enabled,
        ));
        completion.append(&setting_row(
            "ntfy server URL",
            "Use https://ntfy.sh or the address of a server on your local network.",
            &ntfy_server_url,
        ));
        completion.append(&setting_row(
            "ntfy topic",
            "Subscribe to this exact topic in the phone app. Choose an unguessable name on public servers.",
            &ntfy_topic,
        ));
        completion.append(&setting_row(
            "ntfy access token",
            "Optional bearer token for a protected topic.",
            &ntfy_access_token,
        ));
        io.append(&completion);

        let dvd = media_page(
            "DVD",
            "DVD-Video",
            "Structure repair and navigation options for authored DVDs.",
        );
        let dvd_reading = section(
            "Disc processing",
            "Automatic is recommended; alternatives help with unusual authoring.",
        );
        dvd_reading.append(&setting_row(
            "Structure protection handling",
            "Select how damaged or deliberately confusing cells are interpreted.",
            &dvd_structure_protection_method,
        ));
        dvd_reading.append(&path_setting_row(
            "Default destination",
            "Optional DVD library folder. Leave empty to inherit the global destination.",
            &dvd_directory,
            dialog,
            "Choose DVD output folder",
            gtk::FileChooserAction::SelectFolder,
        ));
        dvd_reading.append(&setting_row(
            "Discovery mode",
            "Controls how authored title and title-set boundaries are presented.",
            &dvd_processing_mode,
        ));
        dvd.append(&dvd_reading);
        dvd.append(&inherited_note(
            "Minimum title length and stream defaults are inherited from Output and Audio & subtitles.",
        ));

        let bluray = media_page(
            "Blu-ray",
            "Blu-ray & UHD",
            "Playlist discovery and diagnostics for Blu-ray media.",
        );
        let playlists = section(
            "Default playlist selection",
            "Every discovered playlist remains visible, regardless of these settings.",
        );
        playlists.append(&setting_row(
            "Select only the authored main title",
            "When the disc identifies a main title, select only that playlist by default. If it does not, the normal selection rules apply.",
            &bluray_select_only_main_title,
        ));
        playlists.append(&setting_row(
            "Select exact duplicate playlists",
            "Select later playlists that contain exactly the same clip ranges. When disabled, they remain visible but start unselected.",
            &bluray_select_duplicate_playlists,
        ));
        playlists.append(&path_setting_row(
            "Default destination",
            "Optional Blu-ray library folder. Leave empty to inherit the global destination.",
            &bluray_directory,
            dialog,
            "Choose Blu-ray output folder",
            gtk::FileChooserAction::SelectFolder,
        ));
        bluray.append(&playlists);
        let bluray_tools = section(
            "Protection diagnostics",
            "Uses system providers installed on this computer.",
        );
        bluray_tools.append(&setting_row(
            "Create protection diagnostics",
            "Keep provider dumps when a supported library reports a failure.",
            &create_protection_dumps,
        ));
        bluray_tools.append(&path_setting_row(
            "Java executable",
            "Optional helper used for BD-J metadata and playlist analysis.",
            &java_executable,
            dialog,
            "Choose Java executable",
            gtk::FileChooserAction::Open,
        ));
        bluray.append(&bluray_tools);

        let audio_cd = media_page(
            "Audio CD",
            "Audio CD · available",
            "Secure extraction and output defaults for music and audiobooks.",
        );
        let cd_output = section(
            "Extraction",
            "Lossless output and verification-friendly reading are the defaults.",
        );
        cd_output.append(&setting_row(
            "Output format",
            "Choose FLAC, uncompressed WAV, compact Opus, or broadly compatible M4A/AAC.",
            &audio_cd_output_format,
        ));
        cd_output.append(&path_setting_row(
            "Default destination",
            "Optional music library folder. Leave empty to inherit the global destination.",
            &audio_cd_directory,
            dialog,
            "Choose Audio CD output folder",
            gtk::FileChooserAction::SelectFolder,
        ));
        cd_output.append(&setting_row(
            "Output path",
            "Template used for Audio CD output. Slashes in this template create folders; slashes in metadata are sanitized. Available fields: {artist}, {album}, {disc}, {track}, {track:02}, and {title}.",
            &audio_cd_file_name_template,
        ));
        cd_output.append(&setting_row(
            "Secure extraction (always on)",
            "Use cdparanoia to re-read uncertain sectors and compare results.",
            &audio_cd_secure_extraction,
        ));
        cd_output.append(&setting_row(
            "Read disc metadata",
            "Use embedded CD-TEXT for album, artist, and track names when available.",
            &audio_cd_use_disc_metadata,
        ));
        cd_output.append(&setting_row(
            "Automatically look up tags",
            "Match the disc with MusicBrainz while it is read, then apply editable album and track tags.",
            &audio_cd_auto_tagging,
        ));
        audio_cd.append(&cd_output);

        let integrations = page(
            "Integrations",
            "System media providers and optional local streaming.",
        );
        let metadata = section(
            "Video metadata",
            "Identify authored DVD and Blu-ray contents while a disc is read.",
        );
        metadata.append(&setting_row(
            "TheDiscDB matching",
            "Identify authored discs using exact content fingerprints; cache successful matches locally.",
            &thediscdb_enabled,
        ));
        metadata.append(&setting_row(
            "Use previous DVD match",
            "If normal identification fails, try the identity and episode position from the last DVD whose metadata you applied and ripped.",
            &use_previous_dvd_match,
        ));
        metadata.append(&setting_row(
            "TVMaze enrichment",
            "Add credential-free TV series details, episode runtimes, and artwork.",
            &tvmaze_enabled,
        ));
        metadata.append(&setting_row(
            "TMDB Read Access Token",
            "Optional. Adds canonical movie/series details and poster artwork; obtain a v4 token from themoviedb.org.",
            &tmdb_access_token,
        ));
        metadata.append(&setting_row(
            "TVDB API key",
            "Optional. Adds a second identity source and DVD episode ordering; obtain a v4 project key from thetvdb.com.",
            &tvdb_api_key,
        ));
        metadata.append(&setting_row(
            "TVDB subscriber PIN",
            "Optional. Required only when the TVDB project key uses their user-supported access model.",
            &tvdb_subscriber_pin,
        ));
        integrations.append(&metadata);
        let providers = section(
            "Optical access",
            "Provider availability depends on libraries installed by the operating system.",
        );
        providers.append(&setting_row(
            "Access provider",
            "Automatic uses libdvdread, libdvdcss, libbluray, libaacs, or libbdplus when available.",
            &optical_access_provider,
        ));
        providers.append(&setting_row(
            "Expose access to media players",
            "Allow compatible local players to use the selected provider.",
            &expose_to_media_players,
        ));
        integrations.append(&providers);
        let streaming = section(
            "Local streaming",
            "Serve the currently opened source to devices on this computer or network.",
        );
        streaming.append(&setting_row(
            "Enable streaming server",
            "The server starts only while MediaRip is running.",
            &streaming_enabled,
        ));
        streaming.append(&setting_row(
            "Listen address",
            "Use 127.0.0.1 to limit access to this computer.",
            &streaming_address,
        ));
        streaming.append(&setting_row(
            "Port",
            "Port used by compatible local clients.",
            &streaming_port,
        ));
        integrations.append(&streaming);

        let advanced = page(
            "Advanced",
            "Profiles, selection rules, naming, and external tools.",
        );
        advanced.append(&warning_note(
            "These values can change which tracks are selected and how files are named. Review the title tree before starting a job.",
        ));
        let profiles = section(
            "Profiles & rules",
            "The profile boundary is ready for additional containers and encoding presets.",
        );
        profiles.append(&setting_row(
            "Default profile",
            "Source quality copies streams without re-encoding.",
            &default_profile,
        ));
        profiles.append(&setting_row(
            "Default selection rule",
            "Comma-separated include, exclude, and weighting rules.",
            &default_selection_rule,
        ));
        profiles.append(&setting_row(
            "Output file name template",
            "Available fields include source and title.",
            &output_file_name_template,
        ));
        advanced.append(&profiles);
        let tools = section(
            "External tools",
            "Leave a path empty to discover the executable from the system.",
        );
        tools.append(&path_setting_row(
            "FFmpeg executable",
            "Used for stream-copy remuxing and encoding profiles.",
            &ffmpeg_executable,
            dialog,
            "Choose FFmpeg executable",
            gtk::FileChooserAction::Open,
        ));
        tools.append(&path_setting_row(
            "CCExtractor executable",
            "Optional closed-caption extraction helper.",
            &ccextractor_executable,
            dialog,
            "Choose CCExtractor executable",
            gtk::FileChooserAction::Open,
        ));
        tools.append(&path_setting_row(
            "Temporary directory",
            "Partial files stay beside their destination unless overridden.",
            &temporary_directory,
            dialog,
            "Choose temporary directory",
            gtk::FileChooserAction::SelectFolder,
        ));
        advanced.append(&tools);

        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("settings-root");
        root.set_vexpand(true);
        root.set_hexpand(true);

        let navigation = gtk::Box::new(gtk::Orientation::Vertical, 4);
        navigation.add_css_class("settings-sidebar");
        navigation.set_size_request(230, -1);
        let navigation_title = gtk::Label::new(Some("MediaRip"));
        navigation_title.set_xalign(0.0);
        navigation_title.add_css_class("settings-sidebar-title");
        navigation.append(&navigation_title);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(140);

        let pages = [
            ("general", "General", "Application", general),
            ("output", "Output", "Folders & filtering", output),
            (
                "selection",
                "Audio & subtitles",
                "Language & defaults",
                selection,
            ),
            ("io", "Drives & I/O", "Reading & completion", io),
            ("dvd", "DVD", "DVD-Video", dvd),
            ("bluray", "Blu-ray", "Blu-ray & UHD", bluray),
            ("audio-cd", "Audio CD", "Extraction defaults", audio_cd),
            (
                "integrations",
                "Integrations",
                "Providers & streaming",
                integrations,
            ),
            ("advanced", "Advanced", "Profiles & tools", advanced),
        ];

        let mut first_button: Option<gtk::ToggleButton> = None;
        for (index, (name, title, subtitle, page)) in pages.into_iter().enumerate() {
            if index == 0 {
                navigation.append(&navigation_group("APPLICATION"));
            } else if index == 4 {
                navigation.append(&navigation_group("MEDIA TYPES"));
            } else if index == 7 {
                navigation.append(&navigation_group("SYSTEM"));
            }

            // Preserve a readable form width on large displays. The navigation
            // remains anchored while each settings page is centred in its pane.
            page.set_halign(gtk::Align::Center);
            let scroller = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vscrollbar_policy(gtk::PolicyType::Automatic)
                .child(&page)
                .build();
            stack.add_named(&scroller, Some(name));

            let button = navigation_button(title, subtitle);
            if let Some(first) = &first_button {
                button.set_group(Some(first));
            } else {
                first_button = Some(button.clone());
                button.set_active(true);
            }
            let stack_for_button = stack.clone();
            let page_name = name.to_owned();
            button.connect_toggled(move |button| {
                if button.is_active() {
                    stack_for_button.set_visible_child_name(&page_name);
                }
            });
            navigation.append(&button);
        }

        let navigation_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&navigation)
            .build();
        root.append(&navigation_scroller);
        root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        root.append(&stack);

        custom_directory.set_sensitive(destination_mode.active_id().as_deref() == Some("custom"));
        let custom_directory_for_mode = custom_directory.clone();
        destination_mode.connect_changed(move |mode| {
            custom_directory_for_mode.set_sensitive(mode.active_id().as_deref() == Some("custom"));
        });

        require_audio_cd_metadata.set_sensitive(audio_cd_auto_save.is_active());
        let require_metadata_for_auto_save = require_audio_cd_metadata.clone();
        audio_cd_auto_save.connect_active_notify(move |toggle| {
            require_metadata_for_auto_save.set_sensitive(toggle.is_active());
        });

        tvdb_subscriber_pin.set_sensitive(!tvdb_api_key.text().trim().is_empty());
        let tvdb_pin_for_key = tvdb_subscriber_pin.clone();
        tvdb_api_key.connect_text_notify(move |key| {
            tvdb_pin_for_key.set_sensitive(!key.text().trim().is_empty());
        });

        set_streaming_controls_sensitive(
            &streaming_address,
            &streaming_port,
            streaming_enabled.is_active(),
        );
        let streaming_address_for_toggle = streaming_address.clone();
        let streaming_port_for_toggle = streaming_port.clone();
        streaming_enabled.connect_active_notify(move |toggle| {
            set_streaming_controls_sensitive(
                &streaming_address_for_toggle,
                &streaming_port_for_toggle,
                toggle.is_active(),
            );
        });

        set_ntfy_controls_sensitive(
            &ntfy_server_url,
            &ntfy_topic,
            &ntfy_access_token,
            ntfy_enabled.is_active(),
        );
        let ntfy_server_for_toggle = ntfy_server_url.clone();
        let ntfy_topic_for_toggle = ntfy_topic.clone();
        let ntfy_token_for_toggle = ntfy_access_token.clone();
        ntfy_enabled.connect_active_notify(move |toggle| {
            set_ntfy_controls_sensitive(
                &ntfy_server_for_toggle,
                &ntfy_topic_for_toggle,
                &ntfy_token_for_toggle,
                toggle.is_active(),
            );
        });

        macro_rules! mark_planned {
            ($($widget:expr),+ $(,)?) => {
                $(
                    $widget.set_sensitive(false);
                    $widget.set_tooltip_text(Some("Planned — not available in this release"));
                )+
            };
        }
        mark_planned!(
            data_directory,
            debug_logging,
            expert_controls,
            av_sync_messages,
            minimum_free_space_gib,
            read_retry_count,
            read_buffer_mib,
            single_drive_mode,
            dvd_structure_protection_method,
            dvd_processing_mode,
            create_protection_dumps,
            java_executable,
            audio_cd_secure_extraction,
            optical_access_provider,
            expose_to_media_players,
            streaming_enabled,
            streaming_address,
            streaming_port,
            default_selection_rule,
            output_file_name_template,
            ffmpeg_executable,
            ccextractor_executable,
            temporary_directory,
        );

        let form = Rc::new(Self {
            data_directory,
            debug_logging,
            expert_controls,
            av_sync_messages,
            allow_online_access,
            destination_mode,
            custom_directory,
            audio_cd_directory,
            dvd_directory,
            bluray_directory,
            minimum_title_seconds,
            minimum_free_space_gib,
            preferred_languages,
            audio_policy,
            subtitle_policy,
            include_forced_subtitles,
            select_tv_episodes,
            select_tv_play_all,
            select_tv_non_episodic,
            read_retry_count,
            read_buffer_mib,
            single_drive_mode,
            eject_when_finished,
            completion_sound_enabled,
            ntfy_enabled,
            ntfy_server_url,
            ntfy_topic,
            ntfy_access_token,
            audio_cd_auto_read,
            audio_cd_auto_save,
            dvd_auto_read,
            dvd_auto_save,
            bluray_auto_read,
            bluray_auto_save,
            require_audio_cd_metadata,
            dvd_structure_protection_method,
            dvd_processing_mode,
            bluray_select_only_main_title,
            bluray_select_duplicate_playlists,
            create_protection_dumps,
            java_executable,
            audio_cd_output_format,
            audio_cd_file_name_template,
            audio_cd_secure_extraction,
            audio_cd_use_disc_metadata,
            audio_cd_auto_tagging,
            thediscdb_enabled,
            use_previous_dvd_match,
            tvmaze_enabled,
            tmdb_access_token,
            tvdb_api_key,
            tvdb_subscriber_pin,
            optical_access_provider,
            expose_to_media_players,
            streaming_enabled,
            streaming_address,
            streaming_port,
            default_profile,
            default_selection_rule,
            output_file_name_template,
            ffmpeg_executable,
            ccextractor_executable,
            temporary_directory,
            initial_settings: RefCell::new(settings.clone()),
            apply_button,
        });
        (root, form)
    }

    fn connect_change_tracking(self: &Rc<Self>) {
        macro_rules! watch {
            ($widget:expr, $property:literal) => {{
                let form = Rc::downgrade(self);
                $widget.connect_notify_local(Some($property), move |_, _| {
                    if let Some(form) = form.upgrade() {
                        form.refresh_apply_sensitivity();
                    }
                });
            }};
        }

        watch!(self.data_directory, "text");
        watch!(self.debug_logging, "active");
        watch!(self.expert_controls, "active");
        watch!(self.av_sync_messages, "active");
        watch!(self.allow_online_access, "active");
        watch!(self.destination_mode, "active-id");
        watch!(self.custom_directory, "text");
        watch!(self.audio_cd_directory, "text");
        watch!(self.dvd_directory, "text");
        watch!(self.bluray_directory, "text");
        watch!(self.minimum_title_seconds, "value");
        watch!(self.minimum_free_space_gib, "value");
        for (_, button) in &self.preferred_languages {
            watch!(button, "active");
        }
        watch!(self.audio_policy, "active-id");
        watch!(self.subtitle_policy, "active-id");
        watch!(self.include_forced_subtitles, "active");
        watch!(self.select_tv_episodes, "active");
        watch!(self.select_tv_play_all, "active");
        watch!(self.select_tv_non_episodic, "active");
        watch!(self.read_retry_count, "value");
        watch!(self.read_buffer_mib, "active-id");
        watch!(self.single_drive_mode, "active");
        watch!(self.eject_when_finished, "active");
        watch!(self.completion_sound_enabled, "active");
        watch!(self.ntfy_enabled, "active");
        watch!(self.ntfy_server_url, "text");
        watch!(self.ntfy_topic, "text");
        watch!(self.ntfy_access_token, "text");
        watch!(self.audio_cd_auto_read, "active");
        watch!(self.audio_cd_auto_save, "active");
        watch!(self.dvd_auto_read, "active");
        watch!(self.dvd_auto_save, "active");
        watch!(self.bluray_auto_read, "active");
        watch!(self.bluray_auto_save, "active");
        watch!(self.require_audio_cd_metadata, "active");
        watch!(self.dvd_structure_protection_method, "active-id");
        watch!(self.dvd_processing_mode, "active-id");
        watch!(self.bluray_select_only_main_title, "active");
        watch!(self.bluray_select_duplicate_playlists, "active");
        watch!(self.create_protection_dumps, "active");
        watch!(self.java_executable, "text");
        watch!(self.audio_cd_output_format, "active-id");
        watch!(self.audio_cd_file_name_template, "text");
        watch!(self.audio_cd_secure_extraction, "active");
        watch!(self.audio_cd_use_disc_metadata, "active");
        watch!(self.audio_cd_auto_tagging, "active");
        watch!(self.thediscdb_enabled, "active");
        watch!(self.use_previous_dvd_match, "active");
        watch!(self.tvmaze_enabled, "active");
        watch!(self.tmdb_access_token, "text");
        watch!(self.tvdb_api_key, "text");
        watch!(self.tvdb_subscriber_pin, "text");
        watch!(self.optical_access_provider, "active-id");
        watch!(self.expose_to_media_players, "active");
        watch!(self.streaming_enabled, "active");
        watch!(self.streaming_address, "text");
        watch!(self.streaming_port, "value");
        watch!(self.default_profile, "active-id");
        watch!(self.default_selection_rule, "text");
        watch!(self.output_file_name_template, "text");
        watch!(self.ffmpeg_executable, "text");
        watch!(self.ccextractor_executable, "text");
        watch!(self.temporary_directory, "text");
    }

    fn refresh_apply_sensitivity(&self) {
        self.apply_button
            .set_sensitive(self.settings() != *self.initial_settings.borrow());
    }

    pub fn mark_applied(&self) {
        self.initial_settings.replace(self.settings());
        self.apply_button.set_sensitive(false);
    }

    pub fn settings(&self) -> AppSettings {
        let mut settings = self.initial_settings.borrow().clone();
        settings.general.data_directory = text(&self.data_directory);
        settings.general.debug_logging = self.debug_logging.is_active();
        settings.general.expert_controls = self.expert_controls.is_active();
        settings.general.av_sync_messages = self.av_sync_messages.is_active();
        settings.general.allow_online_access = self.allow_online_access.is_active();

        settings.output.destination_mode = active_id(&self.destination_mode, "automatic");
        settings.output.custom_directory = text(&self.custom_directory);
        settings.output.audio_cd_directory = text(&self.audio_cd_directory);
        settings.output.dvd_directory = text(&self.dvd_directory);
        settings.output.bluray_directory = text(&self.bluray_directory);
        settings.output.minimum_title_seconds = spin_value(&self.minimum_title_seconds);
        settings.output.minimum_free_space_gib = spin_value(&self.minimum_free_space_gib);

        settings.selection.preferred_languages = self
            .preferred_languages
            .iter()
            .filter(|(_, button)| button.is_active())
            .map(|(id, _)| id.clone())
            .collect();
        settings.selection.audio_policy = active_id(&self.audio_policy, "all");
        settings.selection.subtitle_policy = active_id(&self.subtitle_policy, "all");
        settings.selection.include_forced_subtitles = self.include_forced_subtitles.is_active();
        settings.selection.select_tv_episodes = self.select_tv_episodes.is_active();
        settings.selection.select_tv_play_all = self.select_tv_play_all.is_active();
        settings.selection.select_tv_non_episodic = self.select_tv_non_episodic.is_active();

        settings.io.read_retry_count = spin_value(&self.read_retry_count);
        settings.io.read_buffer_mib = active_id(&self.read_buffer_mib, "0")
            .parse()
            .unwrap_or_default();
        settings.io.single_drive_mode = self.single_drive_mode.is_active();
        settings.io.eject_when_finished = self.eject_when_finished.is_active();

        settings.notifications.completion_sound_enabled = self.completion_sound_enabled.is_active();
        settings.notifications.ntfy_enabled = self.ntfy_enabled.is_active();
        settings.notifications.ntfy_server_url = text(&self.ntfy_server_url);
        settings.notifications.ntfy_topic = text(&self.ntfy_topic);
        settings.notifications.ntfy_access_token = self.ntfy_access_token.text().trim().to_owned();

        settings.automation.audio_cd.read_automatically = self.audio_cd_auto_read.is_active();
        settings.automation.audio_cd.save_automatically = self.audio_cd_auto_save.is_active();
        settings.automation.dvd.read_automatically = self.dvd_auto_read.is_active();
        settings.automation.dvd.save_automatically = self.dvd_auto_save.is_active();
        settings.automation.bluray.read_automatically = self.bluray_auto_read.is_active();
        settings.automation.bluray.save_automatically = self.bluray_auto_save.is_active();
        settings.automation.require_audio_cd_metadata = self.require_audio_cd_metadata.is_active();

        settings.dvd.structure_protection_method =
            active_id(&self.dvd_structure_protection_method, "automatic");
        settings.dvd.processing_mode = active_id(&self.dvd_processing_mode, "automatic");

        settings.bluray.select_only_main_title = self.bluray_select_only_main_title.is_active();
        settings.bluray.select_duplicate_playlists =
            self.bluray_select_duplicate_playlists.is_active();
        settings.bluray.create_protection_dumps = self.create_protection_dumps.is_active();
        settings.bluray.java_executable = text(&self.java_executable);

        settings.audio_cd.output_format = active_id(&self.audio_cd_output_format, "flac");
        settings.audio_cd.file_name_template = text(&self.audio_cd_file_name_template);
        settings.audio_cd.secure_extraction = self.audio_cd_secure_extraction.is_active();
        settings.audio_cd.use_disc_metadata = self.audio_cd_use_disc_metadata.is_active();
        settings.audio_cd.auto_tagging = self.audio_cd_auto_tagging.is_active();

        settings.integrations.thediscdb_enabled = self.thediscdb_enabled.is_active();
        settings.integrations.use_previous_dvd_match = self.use_previous_dvd_match.is_active();
        settings.integrations.tvmaze_enabled = self.tvmaze_enabled.is_active();
        settings.integrations.tmdb_access_token = self.tmdb_access_token.text().to_string();
        settings.integrations.tvdb_api_key = self.tvdb_api_key.text().to_string();
        settings.integrations.tvdb_subscriber_pin = self.tvdb_subscriber_pin.text().to_string();
        settings.integrations.optical_access_provider =
            active_id(&self.optical_access_provider, "system");
        settings.integrations.expose_to_media_players = self.expose_to_media_players.is_active();
        settings.integrations.streaming_enabled = self.streaming_enabled.is_active();
        settings.integrations.streaming_address = text(&self.streaming_address);
        settings.integrations.streaming_port = spin_value(&self.streaming_port);

        settings.advanced.default_profile = active_id(&self.default_profile, "copy-mkv");
        settings.advanced.default_selection_rule = text(&self.default_selection_rule);
        settings.advanced.output_file_name_template = text(&self.output_file_name_template);
        settings.advanced.ffmpeg_executable = text(&self.ffmpeg_executable);
        settings.advanced.ccextractor_executable = text(&self.ccextractor_executable);
        settings.advanced.temporary_directory = text(&self.temporary_directory);
        settings
    }
}

fn page(title: &str, subtitle: &str) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 18);
    root.add_css_class("settings-page");

    let header = gtk::Box::new(gtk::Orientation::Vertical, 3);
    let title = gtk::Label::new(Some(&translate(title)));
    title.set_xalign(0.0);
    title.add_css_class("settings-page-title");
    let subtitle = gtk::Label::new(Some(&translate(subtitle)));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class("settings-page-subtitle");
    header.append(&title);
    header.append(&subtitle);
    root.append(&header);
    root
}

fn media_page(title: &str, badge: &str, subtitle: &str) -> gtk::Box {
    let root = page(title, subtitle);
    let badge = gtk::Label::new(Some(&translate(badge)));
    badge.set_halign(gtk::Align::Start);
    badge.add_css_class("settings-badge");
    root.insert_child_after(&badge, root.first_child().as_ref());
    root
}

fn section(title: &str, subtitle: &str) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("settings-section");

    let header = gtk::Box::new(gtk::Orientation::Vertical, 2);
    header.add_css_class("settings-section-header");
    let title = gtk::Label::new(Some(&translate(title)));
    title.set_xalign(0.0);
    title.add_css_class("settings-section-title");
    let subtitle = gtk::Label::new(Some(&translate(subtitle)));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class("settings-section-subtitle");
    header.append(&title);
    header.append(&subtitle);
    root.append(&header);
    root
}

fn setting_row(title: &str, subtitle: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 18);
    row.add_css_class("settings-row");
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.set_size_request(280, -1);
    let title = gtk::Label::new(Some(&translate(title)));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("settings-row-title");
    let subtitle = gtk::Label::new(Some(&translate(subtitle)));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    subtitle.set_hexpand(true);
    subtitle.set_max_width_chars(48);
    subtitle.add_css_class("settings-row-detail");
    labels.append(&title);
    labels.append(&subtitle);
    row.append(&labels);

    // Keep controls beside their labels instead of pushing them to the far edge
    // when a settings window is maximised.
    let control_column = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    control_column.set_size_request(328, -1);
    control_column.set_halign(gtk::Align::Start);
    control.set_valign(gtk::Align::Center);
    control_column.append(control);
    row.append(&control_column);
    row
}

fn path_setting_row(
    title: &str,
    subtitle: &str,
    entry: &gtk::Entry,
    parent: &gtk::Dialog,
    chooser_title: &'static str,
    action: gtk::FileChooserAction,
) -> gtk::Box {
    let control = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    control.set_size_request(328, -1);
    entry.set_hexpand(true);
    let browse = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text(translate("Browse"))
        .build();
    browse.add_css_class("flat");
    control.append(entry);
    control.append(&browse);

    let parent = parent.clone();
    let entry = entry.clone();
    browse.connect_clicked(move |_| {
        let chooser = gtk::FileChooserNative::builder()
            .title(translate(chooser_title))
            .transient_for(&parent)
            .modal(true)
            .action(action)
            .accept_label(translate("Select"))
            .cancel_label(translate("Cancel"))
            .build();
        let entry_for_response = entry.clone();
        chooser.connect_response(move |chooser, response| {
            if response == gtk::ResponseType::Accept
                && let Some(path) = chooser.file().and_then(|file| file.path())
            {
                entry_for_response.set_text(&path.to_string_lossy());
            }
            chooser.destroy();
        });
        chooser.show();
    });

    setting_row(title, subtitle, &control)
}

fn navigation_group(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&translate(title)));
    label.set_xalign(0.0);
    label.add_css_class("settings-nav-group");
    label
}

fn navigation_button(title: &str, subtitle: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::new();
    button.add_css_class("settings-nav-button");
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let title = gtk::Label::new(Some(&translate(title)));
    title.set_xalign(0.0);
    title.add_css_class("settings-nav-title");
    let subtitle = gtk::Label::new(Some(&translate(subtitle)));
    subtitle.set_xalign(0.0);
    subtitle.add_css_class("settings-nav-detail");
    labels.append(&title);
    labels.append(&subtitle);
    button.set_child(Some(&labels));
    button
}

fn inherited_note(message: &str) -> gtk::Box {
    note(message, "settings-note")
}

fn warning_note(message: &str) -> gtk::Box {
    note(message, "settings-warning-note")
}

fn note(message: &str, class: &str) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    root.add_css_class(class);
    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_valign(gtk::Align::Start);
    let label = gtk::Label::new(Some(&translate(message)));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_hexpand(true);
    root.append(&icon);
    root.append(&label);
    root
}

fn combo(options: &[(&str, &str)], selected: &str) -> gtk::ComboBoxText {
    let combo = gtk::ComboBoxText::new();
    combo.set_size_request(260, -1);
    for (id, label) in options {
        combo.append(Some(id), &translate(label));
    }
    if !combo.set_active_id(Some(selected)) {
        combo.set_active(Some(0));
    }
    combo
}

fn language_checklist(selected: &[String]) -> (gtk::Grid, Vec<(String, gtk::CheckButton)>) {
    const LANGUAGES: [(&str, &str); 10] = [
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

    let grid = gtk::Grid::builder()
        .column_spacing(18)
        .row_spacing(4)
        .width_request(328)
        .build();
    grid.add_css_class("settings-language-list");
    let mut buttons = Vec::with_capacity(LANGUAGES.len());
    for (index, (id, label)) in LANGUAGES.into_iter().enumerate() {
        let button = gtk::CheckButton::with_label(&translate(label));
        button.set_active(selected.iter().any(|selected| selected == id));
        button.set_hexpand(true);
        button.set_halign(gtk::Align::Start);
        grid.attach(&button, (index % 2) as i32, (index / 2) as i32, 1, 1);
        buttons.push((id.to_owned(), button));
    }
    (grid, buttons)
}

fn entry(value: &str, placeholder: &str) -> gtk::Entry {
    gtk::Entry::builder()
        .text(value)
        .placeholder_text(translate(placeholder))
        .width_request(260)
        .build()
}

fn switch(active: bool) -> gtk::Switch {
    gtk::Switch::builder()
        .active(active)
        .halign(gtk::Align::End)
        .build()
}

fn spin(minimum: f64, maximum: f64, step: f64, value: f64) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(minimum, maximum, step);
    spin.set_size_request(120, -1);
    spin.set_value(value);
    spin
}

fn active_id(combo: &gtk::ComboBoxText, fallback: &str) -> String {
    combo
        .active_id()
        .map(|id| id.to_string())
        .unwrap_or_else(|| fallback.to_owned())
}

fn text(entry: &gtk::Entry) -> String {
    entry.text().trim().to_owned()
}

fn spin_value(spin: &gtk::SpinButton) -> u32 {
    spin.value_as_int().max(0) as u32
}

fn set_streaming_controls_sensitive(address: &gtk::Entry, port: &gtk::SpinButton, sensitive: bool) {
    address.set_sensitive(sensitive);
    port.set_sensitive(sensitive);
}

fn set_ntfy_controls_sensitive(
    server_url: &gtk::Entry,
    topic: &gtk::Entry,
    access_token: &gtk::PasswordEntry,
    sensitive: bool,
) {
    server_url.set_sensitive(sensitive);
    topic.set_sensitive(sensitive);
    access_token.set_sensitive(sensitive);
}
