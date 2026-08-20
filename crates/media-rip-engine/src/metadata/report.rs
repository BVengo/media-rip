use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use media_rip_core::{StreamKind, Title, VideoMetadataSuggestion, util::sanitize_file_name};

use crate::diagnostics::MetadataPayload;

use super::VideoEpisode;

const MIN_EPISODE_MS: u64 = 20 * 60_000;
const DEFAULT_MAX_EPISODE_MS: u64 = 75 * 60_000;
const ABSOLUTE_MAX_EPISODE_MS: u64 = 120 * 60_000;
const EXPORT_DIRECTORY_ENV: &str = "MEDIA_RIP_EPISODE_REPORT_DIR";

pub(super) struct EpisodeAlignmentExport<'a> {
    pub selected_show: &'a str,
    pub season: Option<u32>,
    pub providers: &'a [&'static str],
    pub episodes: &'a [VideoEpisode],
    pub provider_episodes: &'a [ProviderEpisodeExport],
    pub titles: &'a [Title],
    pub suggestions: &'a [VideoMetadataSuggestion],
    pub raw_payloads: &'a [MetadataPayload],
}

pub(super) struct ProviderEpisodeExport {
    pub provider: String,
    pub selected_show: String,
    pub episodes: Vec<VideoEpisode>,
}

pub(super) fn export_enabled() -> bool {
    env::var_os(EXPORT_DIRECTORY_ENV).is_some_and(|value| !value.is_empty())
}

pub(super) fn export_episode_alignment(export: EpisodeAlignmentExport<'_>) {
    let Some(directory) = env::var_os(EXPORT_DIRECTORY_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return;
    };
    if let Err(error) = fs::create_dir_all(&directory) {
        log::warn!(
            target: "media_rip_engine::episode_alignment_export",
            "could not create episode spreadsheet directory {}: {error}",
            directory.display()
        );
        return;
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let show = sanitize_file_name(export.selected_show);
    let season = export.season.map_or_else(
        || "season-unknown".to_owned(),
        |season| format!("s{season:02}"),
    );
    let base = format!("episode-alignment-{show}-{season}-{timestamp_ms}");
    let maximum_episode_ms = maximum_episode_title_ms(export.episodes);
    let raw_json = serde_json::to_string_pretty(export.raw_payloads)
        .unwrap_or_else(|error| format!("{{\"serialization_error\":{error:?}}}"));
    let normalized_json = serde_json::to_string_pretty(&serde_json::json!({
        "notice": "This is MediaRip's normalized model. The sibling raw-api.json file is canonical for fields MediaRip does not currently deserialize.",
        "selected_show": export.selected_show,
        "season": export.season,
        "providers": export.providers,
        "selected_episodes": export.episodes.iter().map(|episode| serde_json::json!({
            "episode_number": episode.episode_number,
            "name": episode.name,
            "runtime_minutes": episode.runtime,
        })).collect::<Vec<_>>(),
        "provider_episodes": export.provider_episodes.iter().map(|provider| serde_json::json!({
            "provider": provider.provider,
            "selected_show": provider.selected_show,
            "episodes": provider.episodes.iter().map(|episode| serde_json::json!({
                "episode_number": episode.episode_number,
                "name": episode.name,
                "runtime_minutes": episode.runtime,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "disc_titles": export.titles,
        "suggestions": export.suggestions,
    }))
    .unwrap_or_else(|error| format!("{{\"serialization_error\":{error:?}}}"));
    let mut files = vec![
        (directory.join(format!("{base}-raw-api.json")), raw_json),
        (
            directory.join(format!("{base}-raw-fields.csv")),
            raw_fields_csv(export.raw_payloads),
        ),
        (
            directory.join(format!("{base}-normalized-scan.json")),
            normalized_json,
        ),
        (
            directory.join(format!("{base}-titles.csv")),
            titles_csv(&export, maximum_episode_ms),
        ),
        (
            directory.join(format!("{base}-episodes.csv")),
            episodes_csv(&export),
        ),
        (
            directory.join(format!("{base}-provider-episodes.csv")),
            provider_episodes_csv(&export),
        ),
    ];
    if !export.episodes.is_empty() {
        files.push((
            directory.join(format!("{base}-alignment.csv")),
            alignment_csv(&export, maximum_episode_ms),
        ));
    }
    let mut written = Vec::new();
    for (path, contents) in files {
        match fs::write(&path, contents) {
            Ok(()) => written.push(path),
            Err(error) => log::warn!(
                target: "media_rip_engine::episode_alignment_export",
                "could not write {}: {error}",
                path.display()
            ),
        }
    }
    if !written.is_empty() {
        log::info!(
            target: "media_rip_engine::episode_alignment_export",
            "wrote manual episode-alignment spreadsheets: {}",
            written
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn raw_fields_csv(payloads: &[MetadataPayload]) -> String {
    let mut csv = Csv::new(&[
        "payload_index",
        "provider",
        "operation",
        "source",
        "json_pointer",
        "value_type",
        "value_json",
    ]);
    for (payload_index, payload) in payloads.iter().enumerate() {
        let mut fields = Vec::new();
        flatten_json("", &payload.payload, &mut fields);
        for (pointer, value_type, value_json) in fields {
            csv.push([
                (payload_index + 1).to_string(),
                payload.provider.clone(),
                payload.operation.clone(),
                payload.source.clone(),
                pointer,
                value_type.to_owned(),
                value_json,
            ]);
        }
    }
    csv.finish()
}

fn flatten_json(
    pointer: &str,
    value: &serde_json::Value,
    fields: &mut Vec<(String, &'static str, String)>,
) {
    match value {
        serde_json::Value::Object(values) if !values.is_empty() => {
            for (key, value) in values {
                let key = key.replace('~', "~0").replace('/', "~1");
                flatten_json(&format!("{pointer}/{key}"), value, fields);
            }
        }
        serde_json::Value::Array(values) if !values.is_empty() => {
            for (index, value) in values.iter().enumerate() {
                flatten_json(&format!("{pointer}/{index}"), value, fields);
            }
        }
        value => fields.push((
            pointer.to_owned(),
            json_value_type(value),
            value.to_string(),
        )),
    }
}

fn json_value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn maximum_episode_title_ms(episodes: &[VideoEpisode]) -> u64 {
    episodes
        .iter()
        .filter_map(|episode| episode.runtime)
        .max()
        .map(|minutes| minutes.saturating_add(8).saturating_mul(60_000))
        .unwrap_or(DEFAULT_MAX_EPISODE_MS)
        .clamp(DEFAULT_MAX_EPISODE_MS, ABSOLUTE_MAX_EPISODE_MS)
}

fn rejection_reasons(title: &Title, maximum_episode_ms: u64) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if title.duration_ms < MIN_EPISODE_MS {
        reasons.push("duration_below_20_minutes");
    }
    if title.duration_ms > maximum_episode_ms {
        reasons.push("duration_above_episode_maximum");
    }
    if title.chapters.len() < 2 {
        reasons.push("fewer_than_two_chapters");
    }
    reasons
}

fn titles_csv(export: &EpisodeAlignmentExport<'_>, maximum_episode_ms: u64) -> String {
    let mut csv = Csv::new(&[
        "title_position",
        "candidate_ordinal",
        "title_id",
        "source_title_id",
        "disc_title_number",
        "name",
        "duration_ms",
        "duration_minutes",
        "chapter_count",
        "track_count",
        "video_track_count",
        "audio_track_count",
        "subtitle_track_count",
        "codecs",
        "languages",
        "segment_count",
        "segment_map",
        "duplicate_key",
        "automatic_candidate",
        "automatic_rejection_reasons",
        "match_status",
        "matching_outcome_explanation",
        "assigned_season",
        "assigned_episode",
        "suggested_season",
        "suggested_episode",
        "suggested_name",
        "evidence",
        "assumptions",
    ]);
    let mut candidate_ordinal = 0_usize;
    for (position, title) in export.titles.iter().enumerate() {
        let reasons = rejection_reasons(title, maximum_episode_ms);
        let candidate = reasons.is_empty();
        if candidate {
            candidate_ordinal += 1;
        }
        let suggestion = suggestion_for(title, export.suggestions);
        let metadata = title.video_metadata.as_ref();
        csv.push([
            (position + 1).to_string(),
            if candidate {
                candidate_ordinal.to_string()
            } else {
                String::new()
            },
            title.id.to_string(),
            title.source_title_id.clone(),
            option(title.disc_title_number),
            title.name.clone(),
            title.duration_ms.to_string(),
            format!("{:.3}", title.duration_ms as f64 / 60_000.0),
            title.chapters.len().to_string(),
            title.tracks.len().to_string(),
            track_count(title, StreamKind::Video).to_string(),
            track_count(title, StreamKind::Audio).to_string(),
            track_count(title, StreamKind::Subtitle).to_string(),
            joined_unique(title.tracks.iter().map(|track| track.codec.as_str())),
            joined_unique(
                title
                    .tracks
                    .iter()
                    .filter_map(|track| track.language.as_deref()),
            ),
            option(title.segment_count),
            title.segment_map.clone().unwrap_or_default(),
            title.duplicate_key.clone().unwrap_or_default(),
            candidate.to_string(),
            reasons.join(" | "),
            match_status(title, suggestion).to_owned(),
            outcome_explanation(title, suggestion, &reasons).to_owned(),
            metadata
                .and_then(|metadata| metadata.season)
                .map_or_else(String::new, |value| value.to_string()),
            metadata
                .and_then(|metadata| metadata.episode)
                .map_or_else(String::new, |value| value.to_string()),
            suggestion
                .and_then(|suggestion| suggestion.metadata.season)
                .map_or_else(String::new, |value| value.to_string()),
            suggestion
                .and_then(|suggestion| suggestion.metadata.episode)
                .map_or_else(String::new, |value| value.to_string()),
            suggestion
                .map(|suggestion| suggestion.name.clone())
                .unwrap_or_default(),
            suggestion
                .map(|suggestion| suggestion.evidence.join(" | "))
                .unwrap_or_default(),
            suggestion
                .map(|suggestion| suggestion.assumptions.join(" | "))
                .unwrap_or_default(),
        ]);
    }
    csv.finish()
}

fn episodes_csv(export: &EpisodeAlignmentExport<'_>) -> String {
    let mut csv = Csv::new(&[
        "episode_ordinal",
        "season",
        "episode_number",
        "name",
        "runtime_minutes",
        "runtime_ms",
        "selected_show",
        "providers",
    ]);
    for (ordinal, episode) in export.episodes.iter().enumerate() {
        csv.push([
            (ordinal + 1).to_string(),
            option(export.season),
            episode.episode_number.to_string(),
            episode.name.clone(),
            option(episode.runtime),
            episode
                .runtime
                .map(|minutes| minutes.saturating_mul(60_000).to_string())
                .unwrap_or_default(),
            export.selected_show.to_owned(),
            export.providers.join(" + "),
        ]);
    }
    csv.finish()
}

fn provider_episodes_csv(export: &EpisodeAlignmentExport<'_>) -> String {
    let mut csv = Csv::new(&[
        "provider",
        "provider_selected_show",
        "provider_episode_ordinal",
        "episode_number",
        "episode_name",
        "runtime_minutes",
        "present_in_selected_episode_list",
        "selected_episode_name",
        "selected_runtime_minutes",
        "name_agrees_with_selected",
        "runtime_delta_minutes",
    ]);
    for provider in export.provider_episodes {
        for (ordinal, episode) in provider.episodes.iter().enumerate() {
            let selected = export
                .episodes
                .iter()
                .find(|selected| selected.episode_number == episode.episode_number);
            let names_agree = selected.map(|selected| {
                super::tmdb::normalize_title(&selected.name)
                    == super::tmdb::normalize_title(&episode.name)
            });
            let runtime_delta = selected
                .and_then(|selected| Some((episode.runtime?, selected.runtime?)))
                .map(|(provider, selected)| i128::from(provider) - i128::from(selected));
            csv.push([
                provider.provider.clone(),
                provider.selected_show.clone(),
                (ordinal + 1).to_string(),
                episode.episode_number.to_string(),
                episode.name.clone(),
                option(episode.runtime),
                selected.is_some().to_string(),
                selected
                    .map(|episode| episode.name.clone())
                    .unwrap_or_default(),
                selected
                    .and_then(|episode| episode.runtime)
                    .map_or_else(String::new, |value| value.to_string()),
                names_agree.map_or_else(String::new, |value| value.to_string()),
                runtime_delta.map_or_else(String::new, |value| value.to_string()),
            ]);
        }
    }
    csv.finish()
}

fn alignment_csv(export: &EpisodeAlignmentExport<'_>, maximum_episode_ms: u64) -> String {
    let mut csv = Csv::new(&[
        "title_position",
        "candidate_ordinal",
        "title_id",
        "source_title_id",
        "title_name",
        "title_duration_ms",
        "title_duration_minutes",
        "chapter_count",
        "automatic_candidate",
        "automatic_rejection_reasons",
        "current_match_status",
        "matching_outcome_explanation",
        "current_episode",
        "episode_ordinal",
        "episode_number",
        "episode_name",
        "episode_runtime_minutes",
        "episode_runtime_ms",
        "signed_duration_delta_ms",
        "absolute_duration_delta_ms",
        "absolute_error_percent",
        "title_to_episode_runtime_ratio",
        "sequence_offset",
        "is_current_pair",
    ]);
    let mut candidate_ordinal = 0_usize;
    for (position, title) in export.titles.iter().enumerate() {
        let reasons = rejection_reasons(title, maximum_episode_ms);
        let candidate = reasons.is_empty();
        if candidate {
            candidate_ordinal += 1;
        }
        let suggestion = suggestion_for(title, export.suggestions);
        let current_episode = title
            .video_metadata
            .as_ref()
            .and_then(|metadata| metadata.episode)
            .or_else(|| suggestion.and_then(|suggestion| suggestion.metadata.episode));
        for (episode_ordinal, episode) in export.episodes.iter().enumerate() {
            let expected_ms = episode
                .runtime
                .map(|minutes| minutes.saturating_mul(60_000));
            let signed_delta = expected_ms
                .map(|expected| i128::from(title.duration_ms).saturating_sub(i128::from(expected)));
            let absolute_delta = expected_ms.map(|expected| title.duration_ms.abs_diff(expected));
            let error_percent = expected_ms
                .filter(|expected| *expected > 0)
                .map(|expected| {
                    title.duration_ms.abs_diff(expected) as f64 * 100.0 / expected as f64
                });
            let ratio = expected_ms
                .filter(|expected| *expected > 0)
                .map(|expected| title.duration_ms as f64 / expected as f64);
            let sequence_offset =
                candidate.then(|| (episode_ordinal + 1) as i128 - candidate_ordinal as i128);
            csv.push([
                (position + 1).to_string(),
                if candidate {
                    candidate_ordinal.to_string()
                } else {
                    String::new()
                },
                title.id.to_string(),
                title.source_title_id.clone(),
                title.name.clone(),
                title.duration_ms.to_string(),
                format!("{:.3}", title.duration_ms as f64 / 60_000.0),
                title.chapters.len().to_string(),
                candidate.to_string(),
                reasons.join(" | "),
                match_status(title, suggestion).to_owned(),
                outcome_explanation(title, suggestion, &reasons).to_owned(),
                option(current_episode),
                (episode_ordinal + 1).to_string(),
                episode.episode_number.to_string(),
                episode.name.clone(),
                option(episode.runtime),
                expected_ms.map_or_else(String::new, |value| value.to_string()),
                signed_delta.map_or_else(String::new, |value| value.to_string()),
                absolute_delta.map_or_else(String::new, |value| value.to_string()),
                error_percent.map_or_else(String::new, |value| format!("{value:.3}")),
                ratio.map_or_else(String::new, |value| format!("{value:.6}")),
                sequence_offset.map_or_else(String::new, |value| value.to_string()),
                (current_episode == Some(episode.episode_number)).to_string(),
            ]);
        }
    }
    csv.finish()
}

fn suggestion_for<'a>(
    title: &Title,
    suggestions: &'a [VideoMetadataSuggestion],
) -> Option<&'a VideoMetadataSuggestion> {
    suggestions
        .iter()
        .find(|suggestion| suggestion.title_id == title.id)
}

fn match_status(title: &Title, suggestion: Option<&VideoMetadataSuggestion>) -> &'static str {
    if title.video_metadata.is_some() {
        "applied"
    } else if suggestion.is_some() {
        "suggested_for_review"
    } else {
        "unmatched"
    }
}

fn outcome_explanation(
    title: &Title,
    suggestion: Option<&VideoMetadataSuggestion>,
    rejection_reasons: &[&str],
) -> &'static str {
    if title.video_metadata.is_some() {
        "metadata_applied"
    } else if suggestion.is_some() {
        "probable_episode_suggestion_created"
    } else if !rejection_reasons.is_empty() {
        "rejected_by_episode_candidate_filters"
    } else {
        "eligible_but_not_selected_by_group_or_alignment"
    }
}

fn track_count(title: &Title, kind: StreamKind) -> usize {
    title
        .tracks
        .iter()
        .filter(|track| track.kind == kind)
        .count()
}

fn joined_unique<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let mut unique = Vec::new();
    for value in values {
        if !value.is_empty() && !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique.join(" | ")
}

fn option(value: Option<impl ToString>) -> String {
    value.map_or_else(String::new, |value| value.to_string())
}

struct Csv {
    value: String,
}

impl Csv {
    fn new(header: &[&str]) -> Self {
        let mut csv = Self {
            value: String::new(),
        };
        csv.push(header.iter().map(|value| (*value).to_owned()));
        csv
    }

    fn push(&mut self, values: impl IntoIterator<Item = String>) {
        if !self.value.is_empty() {
            self.value.push('\n');
        }
        self.value.push_str(
            &values
                .into_iter()
                .map(|value| escape_csv(&value))
                .collect::<Vec<_>>()
                .join(","),
        );
    }

    fn finish(mut self) -> String {
        self.value.push('\n');
        self.value
    }
}

fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use media_rip_core::{Chapter, StreamKind, Track};

    use super::*;

    fn title(id: u32, duration_ms: u64, chapters: usize) -> Title {
        Title {
            id,
            source_title_id: format!("title-{id}"),
            disc_title_number: Some(id + 1),
            name: format!("Disc title {id}"),
            is_main_title: false,
            duration_ms,
            size_bytes: None,
            size_is_estimate: false,
            chapters: (0..chapters)
                .map(|index| Chapter {
                    index: index as u32,
                    start_ms: index as u64 * 60_000,
                    end_ms: (index as u64 + 1) * 60_000,
                    name: None,
                })
                .collect(),
            tracks: vec![Track {
                index: 1,
                kind: StreamKind::Audio,
                codec: "ac3".to_owned(),
                codec_long_name: None,
                language: Some("eng".to_owned()),
                name: None,
                properties: Vec::new(),
                selected: true,
            }],
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            video_metadata: None,
            output_file: String::new(),
            selected: true,
        }
    }

    #[test]
    fn csv_escapes_spreadsheet_fields_losslessly() {
        assert_eq!(
            escape_csv("Pilot, Part \"One\""),
            "\"Pilot, Part \"\"One\"\"\""
        );
        assert_eq!(escape_csv("plain"), "plain");
    }

    #[test]
    fn alignment_sheet_crosses_rejected_titles_with_every_episode() {
        let titles = vec![title(0, 42 * 60_000, 4), title(1, 5 * 60_000, 1)];
        let episodes = vec![
            VideoEpisode {
                episode_number: 4,
                name: "Episode Four".to_owned(),
                runtime: Some(42),
            },
            VideoEpisode {
                episode_number: 5,
                name: "Episode Five".to_owned(),
                runtime: Some(43),
            },
        ];
        let export = EpisodeAlignmentExport {
            selected_show: "Example Show",
            season: Some(1),
            providers: &["TVDB"],
            episodes: &episodes,
            provider_episodes: &[],
            titles: &titles,
            suggestions: &[],
            raw_payloads: &[],
        };

        let csv = alignment_csv(&export, maximum_episode_title_ms(&episodes));

        assert_eq!(csv.lines().count(), 5);
        assert!(csv.contains("duration_below_20_minutes | fewer_than_two_chapters"));
        assert!(csv.contains("Episode Four"));
        assert!(csv.contains("Episode Five"));
    }

    #[test]
    fn provider_episode_sheet_exposes_episodes_missing_from_selected_list() {
        let selected = vec![VideoEpisode {
            episode_number: 4,
            name: "Episode Four".to_owned(),
            runtime: Some(42),
        }];
        let provider_episodes = vec![ProviderEpisodeExport {
            provider: "TVDB".to_owned(),
            selected_show: "Example Show".to_owned(),
            episodes: vec![
                selected[0].clone(),
                VideoEpisode {
                    episode_number: 5,
                    name: "Episode Five".to_owned(),
                    runtime: Some(43),
                },
            ],
        }];
        let export = EpisodeAlignmentExport {
            selected_show: "Example Show",
            season: Some(1),
            providers: &["TVDB"],
            episodes: &selected,
            provider_episodes: &provider_episodes,
            titles: &[],
            suggestions: &[],
            raw_payloads: &[],
        };

        let csv = provider_episodes_csv(&export);

        assert!(csv.contains("Episode Four,42,true"));
        assert!(csv.contains("Episode Five,43,false"));
    }

    #[test]
    fn raw_field_sheet_flattens_unknown_nested_api_fields_without_dropping_them() {
        let payloads = vec![MetadataPayload {
            provider: "ExampleAPI".to_owned(),
            operation: "season lookup".to_owned(),
            source: "network".to_owned(),
            payload: serde_json::json!({
                "known": 1,
                "currently_unused": {
                    "nested": ["potentially helpful", null, {}]
                }
            }),
        }];

        let csv = raw_fields_csv(&payloads);

        assert!(csv.contains("/currently_unused/nested/0,string"));
        assert!(csv.contains("potentially helpful"));
        assert!(csv.contains("/currently_unused/nested/1,null,null"));
        assert!(csv.contains("/currently_unused/nested/2,object,{}"));
    }
}
