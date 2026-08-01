use std::{
    fs,
    path::{Path, PathBuf},
};

use media_rip_core::{MediaCatalog, StreamKind, Track};
use serde::{Deserialize, Deserializer, Serialize};

const SETTINGS_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppSettings {
    pub version: u32,
    pub general: GeneralSettings,
    pub output: OutputSettings,
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
    pub minimum_title_seconds: u32,
    pub minimum_free_space_gib: u32,
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
    pub secure_extraction: bool,
    pub use_disc_metadata: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct IntegrationSettings {
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
            minimum_title_seconds: 120,
            minimum_free_space_gib: 4,
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
            secure_extraction: true,
            use_disc_metadata: true,
        }
    }
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        Self {
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
        fs::rename(&temporary, &path)
            .map_err(|error| format!("could not replace {}: {error}", path.display()))?;
        Ok(path)
    }

    pub fn apply_catalog_defaults(&self, catalog: &mut MediaCatalog) {
        self.apply_audio_cd_metadata_preference(catalog);
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

    fn apply_audio_cd_metadata_preference(&self, catalog: &mut MediaCatalog) {
        if catalog.source.kind != media_rip_core::DiscKind::AudioCd
            || self.audio_cd.use_disc_metadata
        {
            return;
        }

        catalog.source.name = "Audio CD".to_owned();
        catalog
            .source_info
            .retain(|field| field.label != "Album artist");
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
            for track in &mut title.tracks {
                if let Some(artist) = track.name.take() {
                    track.properties.retain(|property| property != &artist);
                }
            }
        }
    }
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
        assert_eq!(catalog.titles[0].output_file, "01 - Track 01.flac");
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
        let expected = audio_cd_catalog();
        let mut catalog = expected.clone();

        settings.apply_catalog_defaults(&mut catalog);

        assert_eq!(catalog, expected);
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
                output_file: "01 - Song.flac".into(),
                selected: true,
            }],
            source_info: vec![
                InfoField::new("Type", "Audio CD"),
                InfoField::new("Name", "Album"),
                InfoField::new("Album artist", "Artist"),
            ],
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
        }
    }

    fn title(source_title_id: &str, duplicate_key: &str) -> Title {
        Title {
            id: 0,
            source_title_id: source_title_id.into(),
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
            output_file: format!("{source_title_id}.mkv"),
            selected: true,
        }
    }
}
