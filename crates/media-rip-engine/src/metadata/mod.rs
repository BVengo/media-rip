mod cache;
mod discdb;
mod embedded;
mod tmdb;

use std::{collections::HashSet, path::Path};

use media_rip_core::{
    CoverArt, DiscKind, MediaTag, MetadataAttribution, ScanOptions, Title, VideoMetadata,
    VideoMetadataSuggestion, ports::CancellationToken, util::sanitize_file_name,
};

use cache::MetadataCache;
use tmdb::{MediaType, TmdbClient, TmdbEpisode, TmdbMatch};

#[derive(Debug, Default)]
pub(crate) struct VideoEnrichment {
    pub source_name: Option<String>,
    pub year: Option<u32>,
    pub content_type: Option<String>,
    pub content_hash: Option<String>,
    pub disc_match_status: Option<String>,
    pub disc_identity: Option<String>,
    pub cover_art: Option<CoverArt>,
    pub attributions: Vec<MetadataAttribution>,
    pub suggestions: Vec<VideoMetadataSuggestion>,
    pub warnings: Vec<String>,
}

pub(crate) fn enrich_video(
    path: &Path,
    kind: DiscKind,
    fallback_name: &str,
    titles: &mut [Title],
    options: &ScanOptions,
    cancellation: &CancellationToken,
) -> VideoEnrichment {
    let mut result = VideoEnrichment::default();
    let embedded = match kind {
        DiscKind::BluRay => embedded::read_bluray(path),
        DiscKind::Dvd => embedded::EmbeddedMetadata {
            disc_name: crate::source::dvd_text_name(path),
            ..Default::default()
        },
        _ => return result,
    };
    if let Some(name) = embedded.disc_name.filter(|name| useful_name(name)) {
        result.source_name = Some(name);
        result.attributions.push(MetadataAttribution::new(
            if kind == DiscKind::BluRay {
                "Blu-ray Disc Library"
            } else {
                "DVD text"
            },
            100,
            "Authored disc metadata",
        ));
    }
    for title in titles.iter_mut() {
        let Some(number) = title.disc_title_number else {
            continue;
        };
        let Some(name) = embedded
            .title_names
            .get(&number)
            .filter(|name| useful_name(name))
        else {
            continue;
        };
        apply_title_name(
            title,
            name,
            VideoMetadata {
                attribution: MetadataAttribution::new(
                    "Blu-ray Disc Library",
                    100,
                    "Authored title number",
                ),
                content_type: None,
                year: None,
                season: None,
                episode: None,
                tmdb_id: None,
            },
            &[],
        );
    }
    result.cover_art = embedded.artwork;

    if !options.auto_tag_video || !options.allow_online_access || cancellation.is_cancelled() {
        return result;
    }

    let cache = MetadataCache::platform_default();
    result.content_hash = discdb::content_hash(path, kind);
    let mut discdb_match = None;
    if let Some(hash) = result.content_hash.as_deref() {
        match discdb::lookup(hash, &cache, cancellation) {
            Ok(Some(found)) => {
                result.disc_match_status = Some("Exact TheDiscDB match".to_owned());
                discdb_match = Some(found);
            }
            Ok(None) => {
                result.disc_match_status = Some("No exact TheDiscDB record".to_owned());
            }
            Err(error) => {
                result.disc_match_status = Some("TheDiscDB lookup unavailable".to_owned());
                result
                    .warnings
                    .push(format!("TheDiscDB lookup failed: {error}"));
            }
        }
    } else {
        result.disc_match_status = Some("Disc layout could not be fingerprinted".to_owned());
    }

    if let Some(matched) = &discdb_match {
        result.source_name = Some(matched.title.clone());
        result.year = matched.year;
        result.content_type = nonempty(&matched.media_type);
        result.attributions.push(MetadataAttribution::new(
            "TheDiscDB",
            98,
            "Exact disc content hash",
        ));
        let pairs = discdb::matched_title_indices(&matched.titles, titles);
        for (mapping_index, title_index) in pairs {
            let mapping = &matched.titles[mapping_index];
            let Some(item) = &mapping.item else {
                continue;
            };
            let season = item.season.as_deref().and_then(|value| value.parse().ok());
            let episode = item.episode.as_deref().and_then(|value| value.parse().ok());
            let display_name = mapped_display_name(item, &matched.title, season, episode);
            if display_name.is_empty() {
                continue;
            }
            let tags = video_tags(
                &display_name,
                &matched.title,
                matched.year,
                season,
                episode,
                matched.tmdb_id,
            );
            let title = &mut titles[title_index];
            if item.item_type.eq_ignore_ascii_case("MainMovie") {
                title.is_main_title = true;
            }
            apply_title_name(
                title,
                &display_name,
                VideoMetadata {
                    attribution: MetadataAttribution::new(
                        "TheDiscDB",
                        98,
                        "Exact disc hash + verified duration/size",
                    ),
                    content_type: nonempty(&item.item_type),
                    year: matched.year,
                    season,
                    episode,
                    tmdb_id: matched.tmdb_id,
                },
                &tags,
            );
        }
        if let Some(slug) = &matched.disc_slug {
            for title in titles
                .iter_mut()
                .filter(|title| title.video_metadata.is_some())
            {
                set_tag(&mut title.tags, "discdb_disc", slug);
            }
        }
    }

    if let Some(token) = options
        .tmdb_access_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let client = TmdbClient::new(token, &cache, cancellation);
        let metadata_query = result
            .source_name
            .as_deref()
            .unwrap_or(fallback_name)
            .to_owned();
        let disc_hint = disc_hint(&metadata_query);
        let mut resolved_season_episodes = None;
        let resolved_tmdb = if let Some(matched) = &discdb_match {
            matched.tmdb_id.and_then(|id| {
                let media_type = if matched.media_type.eq_ignore_ascii_case("Series") {
                    MediaType::Tv
                } else {
                    MediaType::Movie
                };
                client
                    .details(id, media_type)
                    .map_err(|error| {
                        result.warnings.push(format!("TMDB lookup failed: {error}"));
                    })
                    .ok()
            })
        } else {
            let query = searchable_disc_name(&metadata_query);
            let lookup = if let Some(hint) = disc_hint {
                client
                    .search_tv_season_exact(&query, hint.season)
                    .map(|found| {
                        found.map(|(matched, episodes)| {
                            resolved_season_episodes = Some(episodes);
                            matched
                        })
                    })
            } else {
                client.search_exact(&query)
            };
            match lookup {
                Ok(found) => found,
                Err(error) => {
                    result.warnings.push(format!("TMDB lookup failed: {error}"));
                    None
                }
            }
        };
        if let Some(tmdb_match) = &resolved_tmdb {
            result.source_name = Some(tmdb_match.title.clone());
            result.year = tmdb_match.year.or(result.year);
            result.content_type = Some(
                match tmdb_match.media_type {
                    MediaType::Movie => "Movie",
                    MediaType::Tv => "Series",
                }
                .to_owned(),
            );
            result.attributions.push(MetadataAttribution::new(
                "TMDB",
                if discdb_match.is_some() { 98 } else { 75 },
                if discdb_match.is_some() {
                    "TheDiscDB external ID"
                } else if disc_hint.is_some() {
                    "Exact normalized title + authored season"
                } else {
                    "Exact normalized title match"
                },
            ));
            if discdb_match.is_none() {
                if tmdb_match.media_type == MediaType::Movie {
                    apply_tmdb_movie_match(titles, tmdb_match);
                } else if let (Some(hint), Some(episodes)) =
                    (disc_hint, resolved_season_episodes.as_deref())
                {
                    let probable = probable_episode_suggestions(titles, tmdb_match, episodes, hint);
                    result.suggestions = probable.suggestions;
                    result.disc_identity = probable.disc_identity;
                }
            } else {
                for title in titles
                    .iter_mut()
                    .filter(|title| title.video_metadata.is_some())
                {
                    if let Some(metadata) = &mut title.video_metadata {
                        metadata.tmdb_id = Some(tmdb_match.id);
                        metadata.year = metadata.year.or(tmdb_match.year);
                    }
                    set_tag(&mut title.tags, "tmdb_id", &tmdb_match.id.to_string());
                }
            }
            if result.cover_art.is_none()
                && let Some(path) = tmdb_match.poster_path.as_deref()
            {
                match client.artwork(path) {
                    Ok(artwork) => result.cover_art = Some(artwork),
                    Err(error) => result
                        .warnings
                        .push(format!("TMDB artwork failed: {error}")),
                }
            }
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiscHint {
    season: u32,
    volume: Option<u32>,
    disc: u32,
}

fn disc_hint(value: &str) -> Option<DiscHint> {
    let normalized = value.replace(['_', '-'], " ").to_ascii_lowercase();
    let mut season = None;
    let mut volume = None;
    let mut disc = None;
    for part in normalized.split_whitespace() {
        if let Some(value) = part
            .strip_prefix("season")
            .filter(|value| !value.is_empty())
        {
            season = value.parse().ok();
        } else if let Some(value) = part.strip_prefix('s').filter(|value| !value.is_empty()) {
            season = value.parse().ok();
        } else if let Some(value) = part
            .strip_prefix("volume")
            .filter(|value| !value.is_empty())
        {
            volume = value.parse().ok();
        } else if let Some(value) = part.strip_prefix("vol").filter(|value| !value.is_empty()) {
            volume = value.parse().ok();
        } else if let Some(value) = part.strip_prefix("disc").filter(|value| !value.is_empty()) {
            disc = value.parse().ok();
        } else if let Some(value) = part.strip_prefix('d').filter(|value| !value.is_empty()) {
            disc = value.parse().ok();
        }
    }
    Some(DiscHint {
        season: season.filter(|value| *value > 0)?,
        volume: volume.filter(|value| *value > 0),
        disc: disc.filter(|value| *value > 0)?,
    })
}

#[derive(Debug, Default)]
struct ProbableSuggestions {
    suggestions: Vec<VideoMetadataSuggestion>,
    disc_identity: Option<String>,
}

fn probable_episode_suggestions(
    titles: &[Title],
    series: &TmdbMatch,
    episodes: &[TmdbEpisode],
    hint: DiscHint,
) -> ProbableSuggestions {
    const MIN_EPISODE_MS: u64 = 20 * 60_000;
    const MAX_EPISODE_MS: u64 = 75 * 60_000;
    let episode_like = |title: &Title| {
        (MIN_EPISODE_MS..=MAX_EPISODE_MS).contains(&title.duration_ms) && title.chapters.len() >= 2
    };
    let mut groups = Vec::<Vec<usize>>::new();
    for (play_all_index, play_all) in titles.iter().enumerate() {
        if play_all.duration_ms <= MAX_EPISODE_MS {
            continue;
        }
        let mut group = Vec::new();
        let mut total = 0_u64;
        for (index, title) in titles.iter().enumerate().skip(play_all_index + 1) {
            if title.duration_ms > MAX_EPISODE_MS {
                break;
            }
            if !episode_like(title) {
                if group.is_empty() {
                    continue;
                }
                break;
            }
            group.push(index);
            total = total.saturating_add(title.duration_ms);
            if group.len() >= 2 && total.abs_diff(play_all.duration_ms) <= 3_000 {
                groups.push(group);
                break;
            }
            if total > play_all.duration_ms.saturating_add(3_000) || group.len() == 8 {
                break;
            }
        }
    }
    if groups.is_empty() {
        let candidates = titles
            .iter()
            .enumerate()
            .filter(|(_, title)| episode_like(title))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if (2..=8).contains(&candidates.len()) {
            groups.push(candidates);
        }
    }
    let mut runtimes = groups
        .iter()
        .flatten()
        .map(|index| titles[*index].duration_ms)
        .collect::<Vec<_>>();
    runtimes.sort_unstable();
    let Some(median_runtime) = runtimes.get(runtimes.len() / 2).copied() else {
        return ProbableSuggestions::default();
    };
    let episodes_per_disc = (180 * 60_000 / median_runtime).clamp(2, 8) as u32;
    let episode_count = episodes.len() as u32;
    let estimated_total_discs = episode_count.div_ceil(episodes_per_disc).max(1);
    let volume_aware_disc = hint.volume.and_then(|volume| {
        let discs_per_volume = estimated_total_discs.div_ceil(volume);
        (hint.disc == discs_per_volume)
            .then(|| volume.saturating_sub(1) * discs_per_volume + hint.disc)
            .filter(|disc| *disc <= estimated_total_discs)
    });
    let overall_disc = volume_aware_disc
        .unwrap_or(hint.disc)
        .min(estimated_total_discs);
    let group_size = groups.first().map_or(0, Vec::len) as u32;
    let base_count = episode_count / estimated_total_discs;
    let larger_discs = episode_count % estimated_total_discs;
    let episodes_before = overall_disc.saturating_sub(1) * base_count
        + overall_disc.saturating_sub(1).min(larger_discs);
    let first_episode = if overall_disc == estimated_total_discs && group_size > 0 {
        episode_count.saturating_sub(group_size) + 1
    } else {
        episodes_before + 1
    };
    let confidence = match (groups.len(), volume_aware_disc.is_some()) {
        (1, true) => 80,
        (1, false) => 68,
        (_, true) => 70,
        (_, false) => 58,
    };
    let method = match (groups.len(), volume_aware_disc.is_some()) {
        (1, true) => {
            "Season/volume/disc label + estimated disc set + DVD play-all group + TMDB order"
        }
        (1, false) => "Season/disc label + DVD play-all group + TMDB episode order",
        (_, true) => {
            "Season/volume/disc label + estimated disc set + alternate DVD play-all groups + TMDB order"
        }
        (_, false) => "Season/disc label + alternate DVD play-all groups + TMDB episode order",
    };
    let mut suggestions = Vec::new();
    for group in groups {
        for (offset, title_index) in group.into_iter().enumerate() {
            let episode_number = first_episode.saturating_add(offset as u32);
            let Some(episode) = episodes
                .iter()
                .find(|episode| episode.episode_number == episode_number)
            else {
                continue;
            };
            let title = &titles[title_index];
            if title.video_metadata.is_some()
                || suggestions
                    .iter()
                    .any(|suggestion: &VideoMetadataSuggestion| suggestion.title_id == title.id)
            {
                continue;
            }
            let name = format!("S{:02}E{episode_number:02} — {}", hint.season, episode.name);
            suggestions.push(VideoMetadataSuggestion {
                title_id: title.id,
                name: name.clone(),
                tags: video_tags(
                    &name,
                    &series.title,
                    series.year,
                    Some(hint.season),
                    Some(episode_number),
                    Some(series.id),
                ),
                metadata: VideoMetadata {
                    attribution: MetadataAttribution::new("TMDB", confidence, method),
                    content_type: Some("ProbableEpisode".to_owned()),
                    year: series.year,
                    season: Some(hint.season),
                    episode: Some(episode_number),
                    tmdb_id: Some(series.id),
                },
            });
        }
    }
    let disc_identity = Some(
        if let (Some(volume), Some(inferred)) = (hint.volume, volume_aware_disc) {
            format!(
                "Authored label: season {}, volume {volume}, disc {}; inferred overall disc {inferred} of {estimated_total_discs}",
                hint.season, hint.disc
            )
        } else {
            format!(
                "Authored label: season {}, disc {}; estimated {estimated_total_discs} disc(s) total",
                hint.season, hint.disc
            )
        },
    );
    ProbableSuggestions {
        suggestions,
        disc_identity,
    }
}

pub(crate) fn assign_output_names(titles: &mut [Title], fallback_stem: &str) {
    let mut names = HashSet::new();
    for (index, title) in titles.iter_mut().enumerate() {
        let base = if title.video_metadata.is_some() {
            sanitize_file_name(&title.name)
        } else {
            format!("{}_t{index:02}", sanitize_file_name(fallback_stem))
        };
        let mut candidate = format!("{base}.mkv");
        if !names.insert(candidate.to_ascii_lowercase()) {
            candidate = format!(
                "{}_{}.mkv",
                base,
                sanitize_file_name(&title.source_title_id)
            );
            names.insert(candidate.to_ascii_lowercase());
        }
        title.output_file = candidate;
    }
}

fn apply_tmdb_movie_match(titles: &mut [Title], matched: &TmdbMatch) {
    if matched.media_type != MediaType::Movie {
        return;
    }
    let runtime_ms = matched.runtime_minutes.map(|minutes| minutes * 60_000);
    let mut candidates = titles
        .iter()
        .enumerate()
        .filter(|(_, title)| {
            runtime_ms.is_some_and(|runtime| title.duration_ms.abs_diff(runtime) <= 5 * 60_000)
                || (runtime_ms.is_none() && title.is_main_title)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return;
    }
    let title = &mut titles[candidates.remove(0)];
    let tags = video_tags(
        &matched.title,
        &matched.title,
        matched.year,
        None,
        None,
        Some(matched.id),
    );
    apply_title_name(
        title,
        &matched.title,
        VideoMetadata {
            attribution: MetadataAttribution::new("TMDB", 80, "Exact title + unique runtime match"),
            content_type: Some("MainMovie".to_owned()),
            year: matched.year,
            season: None,
            episode: None,
            tmdb_id: Some(matched.id),
        },
        &tags,
    );
    title.is_main_title = true;
}

fn mapped_display_name(
    item: &discdb::DiscDbItem,
    media_title: &str,
    season: Option<u32>,
    episode: Option<u32>,
) -> String {
    if item.item_type.eq_ignore_ascii_case("Episode") {
        match (season, episode, nonempty(&item.title)) {
            (Some(season), Some(episode), Some(title)) => {
                format!("S{season:02}E{episode:02} — {title}")
            }
            (Some(season), Some(episode), None) => format!("S{season:02}E{episode:02}"),
            (_, _, Some(title)) => title,
            _ => String::new(),
        }
    } else {
        nonempty(&item.title).unwrap_or_else(|| {
            if item.item_type.eq_ignore_ascii_case("MainMovie") {
                media_title.to_owned()
            } else {
                String::new()
            }
        })
    }
}

fn apply_title_name(title: &mut Title, name: &str, metadata: VideoMetadata, tags: &[MediaTag]) {
    title.name = name.to_owned();
    title.video_metadata = Some(metadata);
    for tag in tags {
        set_tag(&mut title.tags, &tag.key, &tag.value);
    }
}

fn video_tags(
    title: &str,
    show: &str,
    year: Option<u32>,
    season: Option<u32>,
    episode: Option<u32>,
    tmdb_id: Option<u64>,
) -> Vec<MediaTag> {
    let mut tags = vec![MediaTag::new("title", title)];
    if season.is_some() || episode.is_some() {
        tags.push(MediaTag::new("show", show));
    }
    if let Some(year) = year {
        tags.push(MediaTag::new("date", year.to_string()));
    }
    if let Some(season) = season {
        tags.push(MediaTag::new("season_number", season.to_string()));
    }
    if let Some(episode) = episode {
        tags.push(MediaTag::new("episode_id", episode.to_string()));
    }
    if let Some(tmdb_id) = tmdb_id {
        tags.push(MediaTag::new("tmdb_id", tmdb_id.to_string()));
    }
    tags
}

fn set_tag(tags: &mut Vec<MediaTag>, key: &str, value: &str) {
    if let Some(tag) = tags
        .iter_mut()
        .find(|tag| tag.key.eq_ignore_ascii_case(key))
    {
        tag.key = key.to_owned();
        tag.value = value.to_owned();
    } else {
        tags.push(MediaTag::new(key, value));
    }
}

fn searchable_disc_name(value: &str) -> String {
    let value = value.replace(['_', '-'], " ");
    let mut parts = value.split_whitespace().collect::<Vec<_>>();
    while parts.last().is_some_and(|part| {
        let normalized = part.to_ascii_lowercase();
        normalized == "bluray"
            || normalized == "blu-ray"
            || normalized == "dvd"
            || normalized.starts_with("volume")
                && normalized[6..].chars().all(|c| c.is_ascii_digit())
            || normalized.starts_with("vol") && normalized[3..].chars().all(|c| c.is_ascii_digit())
            || normalized.starts_with("disc")
            || normalized.starts_with('d') && normalized[1..].chars().all(|c| c.is_ascii_digit())
            || normalized.starts_with('s') && normalized[1..].chars().all(|c| c.is_ascii_digit())
            || normalized.starts_with('s')
                && normalized.contains('d')
                && normalized[1..]
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == 'd')
    }) {
        parts.pop();
    }
    parts.join(" ")
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn useful_name(value: &str) -> bool {
    let normalized = value.trim();
    !normalized.is_empty()
        && !matches!(
            normalized.to_ascii_lowercase().as_str(),
            "unknown" | "untitled"
        )
}

#[cfg(test)]
mod tests {
    use media_rip_core::{Chapter, MetadataAttribution, Title, VideoMetadata};

    use super::{
        DiscHint, MediaType, TmdbEpisode, TmdbMatch, assign_output_names, disc_hint,
        probable_episode_suggestions, searchable_disc_name,
    };

    fn title(name: &str, enriched: bool) -> Title {
        Title {
            id: 0,
            source_title_id: "00800".into(),
            disc_title_number: None,
            name: name.into(),
            is_main_title: false,
            duration_ms: 1,
            size_bytes: None,
            size_is_estimate: false,
            chapters: Vec::new(),
            tracks: Vec::new(),
            segment_count: None,
            segment_map: None,
            duplicate_key: None,
            tags: Vec::new(),
            video_metadata: enriched.then(|| VideoMetadata {
                attribution: MetadataAttribution::new("test", 100, "test"),
                content_type: None,
                year: None,
                season: None,
                episode: None,
                tmdb_id: None,
            }),
            output_file: String::new(),
            selected: false,
        }
    }

    #[test]
    fn enriched_names_become_safe_unique_output_names() {
        let mut titles = vec![title("S01E01 — Pilot", true), title("S01E01 — Pilot", true)];
        titles[1].source_title_id = "00801".into();

        assign_output_names(&mut titles, "DISC");

        assert_eq!(titles[0].output_file, "S01E01 — Pilot.mkv");
        assert_eq!(titles[1].output_file, "S01E01 — Pilot_00801.mkv");
    }

    #[test]
    fn strips_common_disc_suffixes_before_searching() {
        assert_eq!(
            searchable_disc_name("BAND_OF_BROTHERS_S1D1"),
            "BAND OF BROTHERS"
        );
        assert_eq!(searchable_disc_name("Inception-DISC1"), "Inception");
        assert_eq!(
            searchable_disc_name("SUPERNATURAL_S2_VOL2_D3"),
            "SUPERNATURAL"
        );
        assert_eq!(
            disc_hint("SUPERNATURAL_S2_VOL2_D3"),
            Some(DiscHint {
                season: 2,
                volume: Some(2),
                disc: 3,
            })
        );
    }

    #[test]
    fn proposes_each_authored_episode_group_without_applying_it() {
        let mut titles = [
            7_153_400, 2_441_680, 2_358_840, 2_353_840, 6_939_560, 2_392_960, 2_275_040, 2_272_520,
        ]
        .into_iter()
        .enumerate()
        .map(|(id, duration)| {
            let mut title = title(&format!("Title {}", id + 1), false);
            title.id = id as u32;
            title.duration_ms = duration;
            if duration < 4_500_000 {
                title.chapters.push(Chapter {
                    index: 1,
                    start_ms: 0,
                    end_ms: duration / 2,
                    name: None,
                });
                title.chapters.push(Chapter {
                    index: 2,
                    start_ms: duration / 2,
                    end_ms: duration,
                    name: None,
                });
            }
            title
        })
        .collect::<Vec<_>>();
        let original_names = titles
            .iter()
            .map(|title| title.name.clone())
            .collect::<Vec<_>>();
        let series = TmdbMatch {
            id: 1_622,
            media_type: MediaType::Tv,
            title: "Supernatural".into(),
            year: Some(2005),
            runtime_minutes: Some(42),
            poster_path: None,
        };
        let episodes = (1..=22)
            .map(|episode_number| TmdbEpisode {
                episode_number,
                name: format!("Episode {episode_number}"),
                runtime: Some(42),
            })
            .collect::<Vec<_>>();

        let probable = probable_episode_suggestions(
            &titles,
            &series,
            &episodes,
            DiscHint {
                season: 2,
                volume: Some(2),
                disc: 3,
            },
        );

        assert_eq!(probable.suggestions.len(), 6);
        assert_eq!(probable.suggestions[0].name, "S02E20 — Episode 20");
        assert_eq!(probable.suggestions[3].name, "S02E20 — Episode 20");
        assert!(
            probable
                .disc_identity
                .as_deref()
                .is_some_and(|identity| identity.contains("disc 6 of 6"))
        );
        assert_eq!(
            titles
                .iter_mut()
                .map(|title| title.name.clone())
                .collect::<Vec<_>>(),
            original_names
        );
    }
}
