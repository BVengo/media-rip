use std::{
    fs,
    path::{Path, PathBuf},
};

use media_rip_core::{
    MediaCatalog, StreamKind, Track,
    util::{ensure_extension, sanitize_file_name},
};
use serde::{Deserialize, Deserializer, Serialize};

const SETTINGS_VERSION: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub version: u32,
    pub general: GeneralSettings,
    pub output: OutputSettings,
    pub automation: AutomationSettings,
    pub selection: SelectionSettings,
    pub io: IoSettings,
    pub dvd: DvdSettings,
    pub bluray: BluraySettings,
    pub audio_cd: AudioCdSettings,
    pub integrations: IntegrationSettings,
    pub advanced: AdvancedSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GeneralSettings {
    pub data_directory: String,
    pub debug_logging: bool,
    pub expert_controls: bool,
    pub av_sync_messages: bool,
    pub allow_online_access: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct OutputSettings {
    pub destination_mode: String,
    pub custom_directory: String,
    pub audio_cd_directory: String,
    pub dvd_directory: String,
    pub bluray_directory: String,
    pub minimum_title_seconds: u32,
    pub minimum_free_space_gib: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AutomationSettings {
    pub audio_cd: MediaAutomationSettings,
    pub dvd: MediaAutomationSettings,
    pub bluray: MediaAutomationSettings,
    pub require_audio_cd_metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct MediaAutomationSettings {
    pub read_automatically: bool,
    pub save_automatically: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SelectionSettings {
    #[serde(
        default,
        alias = "preferred_language",
        deserialize_with = "deserialize_preferred_languages"
    )]
    pub preferred_languages: Vec<String>,
    pub audio_policy: String,
    pub subtitle_policy: String,
    pub include_forced_subtitles: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct IoSettings {
    pub read_retry_count: u32,
    pub read_buffer_mib: u32,
    pub single_drive_mode: bool,
    pub eject_when_finished: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DvdSettings {
    pub structure_protection_method: String,
    pub processing_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct BluraySettings {
    pub playlist_policy: String,
    pub create_protection_dumps: bool,
    pub java_executable: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AudioCdSettings {
    pub output_format: String,
    pub file_name_template: String,
    pub secure_extraction: bool,
    pub use_disc_metadata: bool,
    pub auto_tagging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct IntegrationSettings {
    pub video_metadata_enabled: bool,
    pub tmdb_access_token: String,
    pub optical_access_provider: String,
    pub expose_to_media_players: bool,
    pub streaming_enabled: bool,
    pub streaming_address: String,
    pub streaming_port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AdvancedSettings {
    pub default_profile: String,
    pub default_selection_rule: String,
    pub output_file_name_template: String,
    pub ffmpeg_executable: String,
    pub ccextractor_executable: String,
    pub temporary_directory: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            general: GeneralSettings::default(),
            output: OutputSettings::default(),
            automation: AutomationSettings::default(),
            selection: SelectionSettings::default(),
            io: IoSettings::default(),
            dvd: DvdSettings::default(),
            bluray: BluraySettings::default(),
            audio_cd: AudioCdSettings::default(),
            integrations: IntegrationSettings::default(),
            advanced: AdvancedSettings::default(),
        }
    }
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            data_directory: String::new(),
            debug_logging: false,
            expert_controls: false,
            av_sync_messages: true,
            allow_online_access: true,
        }
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            destination_mode: "automatic".into(),
            custom_directory: String::new(),
            audio_cd_directory: String::new(),
            dvd_directory: String::new(),
            bluray_directory: String::new(),
            minimum_title_seconds: 120,
            minimum_free_space_gib: 4,
        }
    }
}

impl Default for AutomationSettings {
    fn default() -> Self {
        Self {
            audio_cd: MediaAutomationSettings::default(),
            dvd: MediaAutomationSettings::default(),
            bluray: MediaAutomationSettings::default(),
            require_audio_cd_metadata: true,
        }
    }
}

impl AutomationSettings {
    pub fn for_kind(&self, kind: media_rip_core::DiscKind) -> Option<&MediaAutomationSettings> {
        match kind {
            media_rip_core::DiscKind::AudioCd => Some(&self.audio_cd),
            media_rip_core::DiscKind::Dvd => Some(&self.dvd),
            media_rip_core::DiscKind::BluRay => Some(&self.bluray),
            _ => None,
        }
    }
}

impl Default for SelectionSettings {
    fn default() -> Self {
        Self {
            preferred_languages: Vec::new(),
            audio_policy: "all".into(),
            subtitle_policy: "all".into(),
            include_forced_subtitles: true,
        }
    }
}

impl Default for IoSettings {
    fn default() -> Self {
        Self {
            read_retry_count: 5,
            read_buffer_mib: 0,
            single_drive_mode: false,
            eject_when_finished: false,
        }
    }
}

impl Default for DvdSettings {
    fn default() -> Self {
        Self {
            structure_protection_method: "automatic".into(),
            processing_mode: "automatic".into(),
        }
    }
}

impl Default for BluraySettings {
    fn default() -> Self {
        Self {
            playlist_policy: "unique".into(),
            create_protection_dumps: false,
            java_executable: String::new(),
        }
    }
}

impl Default for AudioCdSettings {
    fn default() -> Self {
        Self {
            output_format: "flac".into(),
            file_name_template: "{artist}/{album}/{disc}-{track:02} - {title}.flac".into(),
            secure_extraction: true,
            use_disc_metadata: true,
            auto_tagging: true,
        }
    }
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        Self {
            video_metadata_enabled: true,
            tmdb_access_token: String::new(),
            optical_access_provider: "system".into(),
            expose_to_media_players: false,
            streaming_enabled: false,
            streaming_address: "127.0.0.1".into(),
            streaming_port: 51000,
        }
    }
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            default_profile: "copy-mkv".into(),
            default_selection_rule:
                "-sel:all,+sel:(favlang|nolang),-sel:(havemulti|havecore),=100:all,-10:favlang"
                    .into(),
            output_file_name_template: "{source}{_t:title:02}".into(),
            ffmpeg_executable: String::new(),
            ccextractor_executable: String::new(),
            temporary_directory: String::new(),
        }
    }
}

impl AppSettings {
    pub fn load() -> Result<Self, String> {
        let path = settings_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        serde_json::from_str(&contents)
            .map_err(|error| format!("could not parse {}: {error}", path.display()))
    }

    pub fn save(&self) -> Result<PathBuf, String> {
        let path = settings_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| "settings path has no parent directory".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        let contents = serde_json::to_string_pretty(self)
            .map_err(|error| format!("could not encode settings: {error}"))?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, format!("{contents}\n"))
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        secure_settings_file(&temporary)?;
        fs::rename(&temporary, &path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(path)
    }

    pub fn apply_catalog_defaults(&self, catalog: &mut MediaCatalog) {
        self.apply_audio_cd_metadata_preference(catalog);
        self.apply_audio_cd_file_names(catalog);
        let had_multiple_titles = catalog.titles.len() > 1;
        let source_kind = catalog.source.kind;
        let duplicate_representatives = duplicate_representatives(catalog);
        let minimum_duration_ms = u64::from(self.output.minimum_title_seconds) * 1_000;
        for title in &mut catalog.titles {
            let is_duplicate_representative = title.duplicate_key.as_ref().is_none_or(|key| {
                duplicate_representatives.get(key) == Some(&title.source_title_id)
            });
            let policy_includes_title = match self.bluray.playlist_policy.as_str() {
                "main" if source_kind == media_rip_core::DiscKind::BluRay => title.is_main_title,
                "all" => true,
                _ => is_duplicate_representative,
            };
            let include_title = policy_includes_title
                && (source_kind == media_rip_core::DiscKind::AudioCd
                    || !had_multiple_titles
                    || title.duration_ms >= minimum_duration_ms);
            for track in &mut title.tracks {
                track.selected = include_title && self.selection.selects_track(track);
            }
            title.selected = include_title
                && (title.tracks.is_empty() || title.tracks.iter().any(|track| track.selected));
        }
    }

    fn apply_audio_cd_file_names(&self, catalog: &mut MediaCatalog) {
        if catalog.source.kind != media_rip_core::DiscKind::AudioCd {
            return;
        }
        for title in &mut catalog.titles {
            title.output_file = format_audio_cd_file_name(
                &self.audio_cd.file_name_template,
                title,
                &catalog.source.name,
            );
        }
    }

    fn apply_audio_cd_metadata_preference(&self, catalog: &mut MediaCatalog) {
        if catalog.source.kind != media_rip_core::DiscKind::AudioCd
            || self.audio_cd.use_disc_metadata
            || catalog.titles.iter().any(|title| {
                title
                    .tags
                    .iter()
                    .any(|tag| tag.key == "musicbrainz_albumid")
            })
        {
            return;
        }

        catalog.source.name = "Audio CD".to_owned();
        catalog
            .source_info
            .retain(|field| field.label != "Album artist");
        if let Some(source) = catalog
            .source_info
            .iter_mut()
            .find(|field| field.label == "Tag source")
        {
            source.value = "Generic track information".to_owned();
        }
        if let Some(name) = catalog
            .source_info
            .iter_mut()
            .find(|field| field.label == "Name")
        {
            name.value.clone_from(&catalog.source.name);
        }

        let width = catalog
            .titles
            .iter()
            .filter_map(|title| title.source_title_id.parse::<u32>().ok())
            .max()
            .unwrap_or_default()
            .to_string()
            .len()
            .max(2);
        for title in &mut catalog.titles {
            let number = title
                .source_title_id
                .parse::<u32>()
                .unwrap_or(title.id.saturating_add(1));
            let generic_name = format!("Track {number:0width$}");
            title.name.clone_from(&generic_name);
            title.output_file = format!("{number:0width$} - {generic_name}.flac");
            title.tags = vec![
                media_rip_core::MediaTag::new("title", &generic_name),
                media_rip_core::MediaTag::new("track", number.to_string()),
            ];
            for track in &mut title.tracks {
                if let Some(artist) = track.name.take() {
                    track.properties.retain(|property| property != &artist);
                }
            }
        }
    }
}

#[cfg(unix)]
fn secure_settings_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("could not secure {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn secure_settings_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

pub(crate) fn format_audio_cd_file_name(
    template: &str,
    title: &media_rip_core::Title,
    album: &str,
) -> String {
    let template = if template.trim().is_empty() {
        "{artist}/{album}/{disc}-{track:02} - {title}.flac"
    } else {
        template
    };
    let tag = |key: &str| {
        title
            .tags
            .iter()
            .find(|tag| tag.key == key)
            .map(|tag| tag.value.as_str())
            .and_then(nonempty)
    };
    let clean_value = |value: &str| {
        if value.is_empty() {
            String::new()
        } else {
            sanitize_file_name(value)
        }
    };
    let track = tag("track")
        .or_else(|| nonempty(&title.source_title_id))
        .map(str::to_owned)
        .unwrap_or_else(|| title.id.saturating_add(1).to_string());
    let track = clean_value(&track);
    let padded_track = track
        .parse::<u32>()
        .map(|number| format!("{number:02}"))
        .unwrap_or_else(|_| track.clone());
    let title_name = tag("title")
        .or_else(|| nonempty(&title.name))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Track {padded_track}"));
    let artist = tag("artist")
        .or_else(|| tag("album_artist"))
        .or_else(|| {
            title
                .tracks
                .first()
                .and_then(|track| track.name.as_deref())
                .and_then(nonempty)
        })
        .unwrap_or("Unknown Artist");
    let album = tag("album")
        .or_else(|| nonempty(album).filter(|album| !album.eq_ignore_ascii_case("Audio CD")))
        .unwrap_or("Unknown Album");
    let disc = tag("discnumber").unwrap_or("1");
    let title_name = clean_value(&title_name);
    let artist = clean_value(artist);
    let album = clean_value(album);
    let disc = clean_value(disc);
    let expanded = template
        // Keep these aliases so a previously entered template continues to work.
        .replace("{disc:1d}", &disc)
        .replace("{disc}", &disc)
        .replace("{track:2d}", &padded_track)
        .replace("{track:02}", &padded_track)
        .replace("{track}", &track)
        .replace("{name}", &title_name)
        .replace("{title}", &title_name)
        .replace("{artist}", &artist)
        .replace("{album}", &album);
    ensure_extension(&expanded, "flac")
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn duplicate_representatives(catalog: &MediaCatalog) -> std::collections::HashMap<String, String> {
    let mut representatives = std::collections::HashMap::new();
    for title in &catalog.titles {
        let Some(key) = &title.duplicate_key else {
            continue;
        };
        representatives
            .entry(key.clone())
            .and_modify(|source_title_id: &mut String| {
                if title.is_main_title {
                    *source_title_id = title.source_title_id.clone();
                }
            })
            .or_insert_with(|| title.source_title_id.clone());
    }
    representatives
}

impl SelectionSettings {
    fn selects_track(&self, track: &Track) -> bool {
        let forced = track
            .properties
            .iter()
            .any(|property| property.eq_ignore_ascii_case("forced only"));
        let default = track
            .properties
            .iter()
            .any(|property| property.eq_ignore_ascii_case("default"));
        let preferred = track
            .language
            .as_deref()
            .is_some_and(|language| self.language_is_preferred(language));
        let language_unknown = track.language.as_deref().is_none_or(language_is_unknown);

        match track.kind {
            StreamKind::Video | StreamKind::Attachment => true,
            StreamKind::Data => false,
            StreamKind::Audio => match self.audio_policy.as_str() {
                "preferred" => self.preferred_languages.is_empty() || preferred || language_unknown,
                "original-preferred" => default || preferred || language_unknown,
                _ => true,
            },
            StreamKind::Subtitle => {
                let selected = match self.subtitle_policy.as_str() {
                    "preferred" => {
                        self.preferred_languages.is_empty() || preferred || language_unknown
                    }
                    "forced" => forced,
                    "none" => false,
                    _ => true,
                };
                selected || (self.include_forced_subtitles && forced)
            }
        }
    }

    fn language_is_preferred(&self, language: &str) -> bool {
        let language = normalize_language(language);
        self.preferred_languages
            .iter()
            .any(|preferred| normalize_language(preferred) == language)
    }
}

fn language_is_unknown(language: &str) -> bool {
    matches!(
        language.trim().to_ascii_lowercase().as_str(),
        "" | "und" | "unknown"
    )
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredLanguages {
    One(String),
    Many(Vec<String>),
}

fn deserialize_preferred_languages<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let stored = StoredLanguages::deserialize(deserializer)?;
    let languages = match stored {
        StoredLanguages::One(language) => vec![language],
        StoredLanguages::Many(languages) => languages,
    };
    let mut normalized = Vec::new();
    for language in languages {
        let language = normalize_language(&language);
        if language != "none" && !language.is_empty() && !normalized.contains(&language) {
            normalized.push(language);
        }
    }
    Ok(normalized)
}

fn normalize_language(language: &str) -> String {
    let primary = language
        .trim()
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match primary.as_str() {
        "en" => "eng",
        "de" | "ger" => "deu",
        "es" => "spa",
        "fr" | "fre" => "fra",
        "it" => "ita",
        "ja" => "jpn",
        "ko" => "kor",
        "pt" => "por",
        "zh" | "chi" => "zho",
        _ => &primary,
    }
    .to_owned()
}

fn settings_path() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(Path::new(&root).join("mediarip").join("settings.json"));
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config").join("mediarip").join("settings.json"))
        .ok_or_else(|| "neither XDG_CONFIG_HOME nor HOME is available".to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use media_rip_core::{
        DiscKind, InfoField, MediaCatalog, MediaSource, StreamKind, Title, Track,
    };

    use super::AppSettings;

    #[test]
    fn settings_round_trip_through_json() {
        let mut expected = AppSettings::default();
        expected.output.minimum_title_seconds = 42;
        expected.selection.preferred_languages = vec!["eng".into(), "jpn".into()];
        expected.audio_cd.output_format = "wav".into();

        let json = serde_json::to_string(&expected).unwrap();
        let actual: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn migrates_the_single_preferred_language_setting() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"selection":{"preferred_language":"eng","audio_policy":"preferred"}}"#,
        )
        .unwrap();

        assert_eq!(settings.selection.preferred_languages, ["eng"]);
    }

    #[test]
    fn preferred_languages_control_audio_and_subtitle_defaults() {
        let mut settings = AppSettings::default();
        settings.selection.preferred_languages = vec!["eng".into(), "jpn".into()];
        settings.selection.audio_policy = "preferred".into();
        settings.selection.subtitle_policy = "preferred".into();

        assert!(
            settings
                .selection
                .selects_track(&track(StreamKind::Audio, Some("en-US"), &[]))
        );
        assert!(
            settings
                .selection
                .selects_track(&track(StreamKind::Subtitle, Some("jpn"), &[]))
        );
        assert!(
            !settings
                .selection
                .selects_track(&track(StreamKind::Audio, Some("fra"), &[]))
        );
        assert!(settings.selection.selects_track(&track(
            StreamKind::Subtitle,
            Some("fra"),
            &["forced only"]
        )));
    }

    #[test]
    fn preferred_policies_keep_tracks_with_unknown_languages() {
        let mut settings = AppSettings::default();
        settings.selection.preferred_languages = vec!["eng".into()];
        settings.selection.audio_policy = "original-preferred".into();
        settings.selection.subtitle_policy = "preferred".into();

        assert!(
            settings
                .selection
                .selects_track(&track(StreamKind::Audio, None, &[]))
        );
        assert!(
            settings
                .selection
                .selects_track(&track(StreamKind::Subtitle, Some("und"), &[]))
        );
        assert!(
            !settings
                .selection
                .selects_track(&track(StreamKind::Audio, Some("fra"), &[]))
        );
    }

    fn track(kind: StreamKind, language: Option<&str>, properties: &[&str]) -> Track {
        Track {
            index: 0,
            kind,
            codec: "TEST".into(),
            codec_long_name: None,
            language: language.map(str::to_owned),
            name: None,
            properties: properties
                .iter()
                .map(|property| (*property).to_owned())
                .collect(),
            selected: false,
        }
    }

    #[test]
    fn missing_fields_keep_forward_compatible_defaults() {
        let settings: AppSettings = serde_json::from_str(
            r#"{
                "output": { "minimum_title_seconds": 300 },
                "io": { "read_retry_count": 9 }
            }"#,
        )
        .unwrap();

        assert_eq!(settings.output.minimum_title_seconds, 300);
        assert_eq!(settings.output.destination_mode, "automatic");
        assert_eq!(settings.io.read_retry_count, 9);
        assert!(settings.general.av_sync_messages);
        assert!(settings.audio_cd.auto_tagging);
        assert!(!settings.automation.audio_cd.read_automatically);
        assert!(settings.automation.require_audio_cd_metadata);
        assert!(settings.output.audio_cd_directory.is_empty());
    }

    #[test]
    fn settings_do_not_store_an_interface_language() {
        let json = serde_json::to_string(&AppSettings::default()).unwrap();

        assert!(!json.contains("interface_language"));
    }

    #[test]
    fn all_playlist_policy_selects_every_exact_duplicate() {
        let mut settings = AppSettings::default();
        settings.bluray.playlist_policy = "all".into();
        let mut catalog = catalog_with_duplicate_playlists();

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog.titles.len(), 3);
        assert!(catalog.titles[0].selected);
        assert!(catalog.titles[1].selected);
        assert!(catalog.titles[1].tracks.iter().all(|track| track.selected));
        assert!(catalog.titles[2].selected);
    }

    #[test]
    fn unique_playlist_policy_shows_duplicates_but_selects_one_representative() {
        let mut settings = AppSettings::default();
        settings.bluray.playlist_policy = "unique".into();
        let mut catalog = catalog_with_duplicate_playlists();

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog.titles.len(), 3);
        assert!(catalog.titles[0].selected);
        assert!(!catalog.titles[1].selected);
        assert!(catalog.titles[2].selected);
    }

    #[test]
    fn exact_duplicate_group_keeps_the_disc_defined_main_title() {
        let settings = AppSettings::default();
        let mut catalog = catalog_with_duplicate_playlists();
        catalog.titles[1].is_main_title = true;

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog.titles.len(), 3);
        assert!(!catalog.titles[0].selected);
        assert!(catalog.titles[1].is_main_title);
        assert!(catalog.titles[1].selected);
    }

    #[test]
    fn configured_minimum_duration_controls_title_defaults() {
        let mut settings = AppSettings::default();
        settings.output.minimum_title_seconds = 8_000;
        let mut catalog = catalog_with_duplicate_playlists();

        settings.apply_catalog_defaults(&mut catalog);

        assert!(catalog.titles.iter().all(|title| !title.selected));
    }

    #[test]
    fn disabling_cd_text_replaces_disc_and_track_names() {
        let mut settings = AppSettings::default();
        settings.audio_cd.use_disc_metadata = false;
        let mut catalog = audio_cd_catalog();

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog.source.name, "Audio CD");
        assert_eq!(catalog.source_info[1].value, "Audio CD");
        assert!(
            catalog
                .source_info
                .iter()
                .all(|field| field.label != "Album artist")
        );
        assert_eq!(catalog.titles[0].name, "Track 01");
        assert_eq!(
            catalog.titles[0].output_file,
            "Unknown Artist/Unknown Album/1-01 - Track 01.flac"
        );
        assert_eq!(catalog.titles[0].tracks[0].name, None);
        assert!(
            !catalog.titles[0].tracks[0]
                .properties
                .contains(&"Artist".into())
        );
    }

    #[test]
    fn enabled_cd_text_preserves_disc_and_track_names() {
        let settings = AppSettings::default();
        let mut expected = audio_cd_catalog();
        expected.titles[0].output_file = "Artist/Album/1-01 - Song.flac".into();
        let mut catalog = expected.clone();

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog, expected);
    }

    #[test]
    fn disabling_cd_text_keeps_an_exact_online_match() {
        let mut settings = AppSettings::default();
        settings.audio_cd.use_disc_metadata = false;
        let mut expected = audio_cd_catalog();
        expected.titles[0].tags.push(media_rip_core::MediaTag::new(
            "musicbrainz_albumid",
            "release-id",
        ));
        expected.titles[0].output_file = "Artist/Album/1-01 - Song.flac".into();
        let mut catalog = expected.clone();

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog, expected);
    }

    #[test]
    fn matched_audio_cd_uses_the_configured_file_name_template() {
        let mut settings = AppSettings::default();
        settings.audio_cd.file_name_template = "{artist} - {track:02} - {title}".into();
        let mut catalog = audio_cd_catalog();
        catalog.titles[0].tags.push(media_rip_core::MediaTag::new(
            "musicbrainz_albumid",
            "release-id",
        ));

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog.titles[0].output_file, "Artist - 01 - Song.flac");
    }

    #[test]
    fn metadata_separators_do_not_create_unconfigured_folders() {
        let mut catalog = audio_cd_catalog();
        catalog.source.name = "Hits/Live".into();
        catalog.titles[0].tracks[0].name = Some("AC/DC".into());
        catalog.titles[0].tags.push(media_rip_core::MediaTag::new(
            "musicbrainz_albumid",
            "release-id",
        ));

        AppSettings::default().apply_catalog_defaults(&mut catalog);

        assert_eq!(
            catalog.titles[0].output_file,
            "AC_DC/Hits_Live/1-01 - Song.flac"
        );
    }

    #[test]
    fn missing_metadata_keeps_the_configured_library_structure() {
        let mut catalog = audio_cd_catalog();
        catalog.source.name.clear();
        catalog.titles[0].name.clear();
        catalog.titles[0].source_title_id.clear();
        catalog.titles[0].tracks[0].name = None;

        AppSettings::default().apply_catalog_defaults(&mut catalog);

        assert_eq!(
            catalog.titles[0].output_file,
            "Unknown Artist/Unknown Album/1-01 - Track 01.flac"
        );
    }

    #[test]
    fn disabling_cd_text_does_not_change_other_media() {
        let mut settings = AppSettings::default();
        settings.audio_cd.use_disc_metadata = false;
        let mut expected = audio_cd_catalog();
        expected.source.kind = DiscKind::File;
        let mut catalog = expected.clone();

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog, expected);
    }

    fn audio_cd_catalog() -> MediaCatalog {
        MediaCatalog {
            source: MediaSource {
                id: "cd".into(),
                name: "Album".into(),
                path: PathBuf::from("/dev/cdrom"),
                kind: DiscKind::AudioCd,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: vec![Title {
                id: 0,
                source_title_id: "1".into(),
                disc_title_number: None,
                name: "Song".into(),
                is_main_title: false,
                duration_ms: 180_000,
                size_bytes: Some(31_752_000),
                size_is_estimate: false,
                chapters: Vec::new(),
                tracks: vec![Track {
                    index: 0,
                    kind: StreamKind::Audio,
                    codec: "PCM".into(),
                    codec_long_name: None,
                    language: None,
                    name: Some("Artist".into()),
                    properties: vec!["stereo".into(), "Artist".into()],
                    selected: true,
                }],
                segment_count: None,
                segment_map: None,
                duplicate_key: None,
                tags: Vec::new(),
                video_metadata: None,
                output_file: "01 - Song.flac".into(),
                selected: true,
            }],
            source_info: vec![
                InfoField::new("Type", "Audio CD"),
                InfoField::new("Name", "Album"),
                InfoField::new("Album artist", "Artist"),
            ],
            cover_art: None,
            track_cover_art: Default::default(),
            metadata_attributions: Vec::new(),
            video_metadata_suggestions: Vec::new(),
        }
    }

    fn catalog_with_duplicate_playlists() -> MediaCatalog {
        MediaCatalog {
            source: MediaSource {
                id: "disc".into(),
                name: "Disc".into(),
                path: PathBuf::from("/disc"),
                kind: DiscKind::BluRay,
                drive: None,
                capacity_bytes: None,
                encrypted: None,
            },
            titles: vec![
                title("00014", "00001:00000000:00015f90;"),
                title("00026", "00001:00000000:00015f90;"),
                title("00111", "00001:00000001:00015f90;"),
            ],
            source_info: Vec::new(),
            cover_art: None,
            track_cover_art: Default::default(),
            metadata_attributions: Vec::new(),
            video_metadata_suggestions: Vec::new(),
        }
    }

    fn title(source_title_id: &str, duplicate_key: &str) -> Title {
        Title {
            id: 0,
            source_title_id: source_title_id.into(),
            disc_title_number: None,
            name: source_title_id.into(),
            is_main_title: false,
            duration_ms: 7_200_000,
            size_bytes: Some(26_900_000_000),
            size_is_estimate: false,
            chapters: Vec::new(),
            tracks: vec![Track {
                index: 0,
                kind: StreamKind::Video,
                codec: "H264".into(),
                codec_long_name: None,
                language: None,
                name: None,
                properties: Vec::new(),
                selected: true,
            }],
            segment_count: Some(1),
            segment_map: Some("00001.m2ts".into()),
            duplicate_key: Some(duplicate_key.into()),
            tags: Vec::new(),
            video_metadata: None,
            output_file: format!("{source_title_id}.mkv"),
            selected: true,
        }
    }
}
