mod cache;
mod discdb;
mod embedded;
mod report;
mod tmdb;
mod tvdb;
mod tvmaze;

use std::{collections::HashSet, path::Path};

use media_rip_core::{
    CoverArt, DiscKind, MediaTag, MetadataAttribution, ScanOptions, Title, VideoEpisodeChoice,
    VideoMetadata, VideoMetadataSuggestion, ports::CancellationToken, util::sanitize_file_name,
};

use cache::MetadataCache;
use tmdb::{MediaType, TmdbClient, TmdbMatch};
use tvdb::{TvdbClient, TvdbMatch, TvdbMediaType};
use tvmaze::{TvmazeClient, TvmazeMatch};

use crate::diagnostics;

#[derive(Debug, Clone, PartialEq, Eq)]
enum VideoMetadataLink {
    Tvmaze {
        show_id: u64,
    },
    Tvdb {
        media_type: TvdbMediaType,
        slug: String,
        season: Option<u32>,
    },
}

impl VideoMetadataLink {
    fn provider(&self) -> &'static str {
        match self {
            Self::Tvmaze { .. } => "TVMaze",
            Self::Tvdb { .. } => "TVDB",
        }
    }

    fn season(&self) -> Option<u32> {
        match self {
            Self::Tvdb { season, .. } => *season,
            Self::Tvmaze { .. } => None,
        }
    }

    fn is_series(&self) -> bool {
        match self {
            Self::Tvmaze { .. } => true,
            Self::Tvdb { media_type, .. } => *media_type == TvdbMediaType::Tv,
        }
    }
}

/// Validate a public metadata page and report the integration it selects.
/// The parsed identity stays private to the engine so callers only need one
/// URL field and do not need provider-specific controls.
pub fn video_metadata_link_provider(value: &str) -> Result<&'static str, String> {
    parse_video_metadata_link(value).map(|link| link.provider())
}

#[derive(Debug, Clone)]
pub struct EpisodeSeasonOptions {
    pub show: String,
    pub season: u32,
    pub year: Option<u32>,
    pub tmdb_id: Option<u64>,
    pub tvdb_id: Option<u64>,
    pub tvmaze_id: Option<u64>,
    pub tmdb_access_token: Option<String>,
    pub tvdb_api_key: Option<String>,
    pub tvdb_subscriber_pin: Option<String>,
    pub tvmaze_enabled: bool,
}

pub fn refresh_episode_season(
    options: EpisodeSeasonOptions,
    cancellation: &CancellationToken,
) -> Result<Vec<VideoEpisodeChoice>, String> {
    let cache = MetadataCache::platform_default();
    let mut provider_results = Vec::new();

    if let (Some(token), Some(id)) = (options.tmdb_access_token.as_deref(), options.tmdb_id) {
        provider_results.push(
            TmdbClient::new(token, &cache, cancellation)
                .season(id, options.season)
                .map(|episodes| {
                    episodes
                        .into_iter()
                        .map(|episode| VideoEpisode {
                            episode_number: episode.episode_number,
                            name: episode.name,
                            runtime: episode.runtime,
                        })
                        .collect::<Vec<_>>()
                }),
        );
    }
    if let (Some(api_key), Some(id)) = (options.tvdb_api_key.as_deref(), options.tvdb_id) {
        provider_results.push(
            TvdbClient::new(
                api_key,
                options.tvdb_subscriber_pin.as_deref(),
                &cache,
                cancellation,
            )
            .season(id, options.season)
            .map(|episodes| {
                episodes
                    .into_iter()
                    .map(|episode| VideoEpisode {
                        episode_number: episode.episode_number,
                        name: episode.name,
                        runtime: episode.runtime,
                    })
                    .collect::<Vec<_>>()
            }),
        );
    }
    if options.tvmaze_enabled
        && let Some(id) = options.tvmaze_id
    {
        provider_results.push(
            TvmazeClient::new(&cache, cancellation)
                .season(id, options.season)
                .map(|episodes| {
                    episodes
                        .into_iter()
                        .map(|episode| VideoEpisode {
                            episode_number: episode.episode_number,
                            name: episode.name,
                            runtime: episode.runtime,
                        })
                        .collect::<Vec<_>>()
                }),
        );
    }

    if provider_results.is_empty() {
        return Err("No enabled metadata provider can load episodes for this series.".to_owned());
    }
    let mut episodes = Vec::new();
    let mut errors = Vec::new();
    for result in provider_results {
        match result {
            Ok(incoming) if episodes.is_empty() => episodes = incoming,
            Ok(incoming) => episodes = merge_episode_metadata(episodes, incoming),
            Err(error) => errors.push(error),
        }
    }
    if episodes.is_empty() {
        return Err(if errors.is_empty() {
            format!("No episodes were found for season {}.", options.season)
        } else {
            errors.join("; ")
        });
    }

    let provider_ids = VideoProviderIds {
        tmdb: options.tmdb_id,
        tvdb: options.tvdb_id,
        tvmaze: options.tvmaze_id,
    };
    Ok(episodes
        .into_iter()
        .map(|episode| {
            let name = format!(
                "{} - S{:02}E{:02} - {}",
                options.show, options.season, episode.episode_number, episode.name
            );
            VideoEpisodeChoice {
                episode: episode.episode_number,
                tags: video_tags(
                    &name,
                    &options.show,
                    options.year,
                    Some(options.season),
                    Some(episode.episode_number),
                    provider_ids,
                ),
                name,
            }
        })
        .collect())
}

fn parse_video_metadata_link(value: &str) -> Result<VideoMetadataLink, String> {
    let value = value.trim();
    let remainder = value
        .strip_prefix("https://")
        .ok_or_else(|| "Metadata link must be an HTTPS TVMaze or TheTVDB URL.".to_owned())?;
    let (authority, path) = remainder.split_once('/').unwrap_or((remainder, ""));
    if authority.contains('@') || authority.contains(':') {
        return Err("Metadata link must use the standard HTTPS provider address.".to_owned());
    }
    let host = authority
        .strip_prefix("www.")
        .unwrap_or(authority)
        .to_ascii_lowercase();
    let segments = path
        .split(['/', '?', '#'])
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    match host.as_str() {
        "tvmaze.com" | "api.tvmaze.com" => {
            let show_position = segments
                .iter()
                .position(|segment| segment.eq_ignore_ascii_case("shows"));
            let show_id = show_position
                .and_then(|position| segments.get(position + 1))
                .and_then(|id| id.parse::<u64>().ok())
                .filter(|id| *id > 0)
                .ok_or_else(|| {
                    "TVMaze link must point to a show page such as https://www.tvmaze.com/shows/82/game-of-thrones."
                        .to_owned()
                })?;
            Ok(VideoMetadataLink::Tvmaze { show_id })
        }
        "thetvdb.com" => {
            let (media_type, prefix) = match segments.first().copied() {
                Some("series") => (TvdbMediaType::Tv, "series"),
                Some("movies") => (TvdbMediaType::Movie, "movies"),
                _ => {
                    return Err("TheTVDB link must point to a series or movie page.".to_owned());
                }
            };
            let slug = segments.get(1).copied().filter(|slug| {
                !slug.is_empty()
                    && slug
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
            });
            let slug = slug
                .ok_or_else(|| format!("TheTVDB link must include a valid {prefix} page name."))?;
            let season = segments.windows(3).find_map(|segments| {
                segments[0]
                    .eq_ignore_ascii_case("seasons")
                    .then(|| segments[2].parse::<u32>().ok())
                    .flatten()
                    .filter(|season| *season > 0)
            });
            Ok(VideoMetadataLink::Tvdb {
                media_type,
                slug: slug.to_owned(),
                season,
            })
        }
        _ => Err("Metadata link must be from tvmaze.com or thetvdb.com.".to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VideoMatch {
    providers: Vec<&'static str>,
    media_type: MediaType,
    title: String,
    year: Option<u32>,
    runtime_minutes: Option<u64>,
    tmdb_id: Option<u64>,
    tvdb_id: Option<u64>,
    tvmaze_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VideoEpisode {
    episode_number: u32,
    name: String,
    runtime: Option<u64>,
}

const MIN_TITLE_REVIEW_SCORE: u8 = 78;
const MIN_TITLE_REVIEW_MARGIN: u8 = 8;

fn title_review_score(query: &str, candidate: &str) -> Option<u8> {
    let query = tmdb::normalize_title(query);
    let candidate = tmdb::normalize_title(candidate);
    if query.is_empty() || candidate.is_empty() {
        return None;
    }
    if query == candidate {
        return Some(100);
    }

    let longest = query.chars().count().max(candidate.chars().count());
    let edit_similarity = 100usize
        .saturating_sub(levenshtein_distance(&query, &candidate).saturating_mul(100) / longest)
        as u8;
    let containment_similarity = if query.contains(&candidate) || candidate.contains(&query) {
        let shortest = query.chars().count().min(candidate.chars().count());
        (70 + shortest.saturating_mul(25) / longest) as u8
    } else {
        0
    };
    let score = edit_similarity.max(containment_similarity);
    (score >= MIN_TITLE_REVIEW_SCORE).then_some(score)
}

fn levenshtein_distance(first: &str, second: &str) -> usize {
    let mut previous = (0..=second.chars().count()).collect::<Vec<_>>();
    let mut current = vec![0; previous.len()];
    for (row, first_character) in first.chars().enumerate() {
        current[0] = row + 1;
        for (column, second_character) in second.chars().enumerate() {
            current[column + 1] = (previous[column + 1] + 1)
                .min(current[column] + 1)
                .min(previous[column] + usize::from(first_character != second_character));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[second.chars().count()]
}

fn clear_title_review_winner(scores: &[u8]) -> bool {
    let Some(best) = scores.first() else {
        return false;
    };
    scores
        .get(1)
        .is_none_or(|runner_up| best.saturating_sub(*runner_up) >= MIN_TITLE_REVIEW_MARGIN)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct VideoProviderIds {
    tmdb: Option<u64>,
    tvdb: Option<u64>,
    tvmaze: Option<u64>,
}

impl From<&VideoMatch> for VideoProviderIds {
    fn from(matched: &VideoMatch) -> Self {
        Self {
            tmdb: matched.tmdb_id,
            tvdb: matched.tvdb_id,
            tvmaze: matched.tvmaze_id,
        }
    }
}

impl From<TmdbMatch> for VideoMatch {
    fn from(matched: TmdbMatch) -> Self {
        Self {
            providers: vec!["TMDB"],
            media_type: matched.media_type,
            title: matched.title,
            year: matched.year,
            runtime_minutes: matched.runtime_minutes,
            tmdb_id: Some(matched.id),
            tvdb_id: None,
            tvmaze_id: None,
        }
    }
}

impl From<TvdbMatch> for VideoMatch {
    fn from(matched: TvdbMatch) -> Self {
        Self {
            providers: vec!["TVDB"],
            media_type: match matched.media_type {
                TvdbMediaType::Movie => MediaType::Movie,
                TvdbMediaType::Tv => MediaType::Tv,
            },
            title: matched.title,
            year: matched.year,
            runtime_minutes: matched.runtime_minutes,
            tmdb_id: None,
            tvdb_id: Some(matched.id),
            tvmaze_id: None,
        }
    }
}

impl From<TvmazeMatch> for VideoMatch {
    fn from(matched: TvmazeMatch) -> Self {
        Self {
            providers: vec!["TVMaze"],
            media_type: MediaType::Tv,
            title: matched.title,
            year: matched.year,
            runtime_minutes: matched.runtime_minutes,
            tmdb_id: None,
            tvdb_id: None,
            tvmaze_id: Some(matched.id),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct VideoEnrichment {
    pub source_name: Option<String>,
    pub year: Option<u32>,
    pub content_type: Option<String>,
    pub content_hash: Option<String>,
    pub disc_match_status: Option<String>,
    pub tvmaze_match_status: Option<String>,
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
                tvdb_id: None,
                tvmaze_id: None,
            },
            &[],
        );
    }
    result.cover_art = embedded.artwork;

    if !options.auto_tag_video || !options.allow_online_access || cancellation.is_cancelled() {
        return result;
    }

    let metadata_link = options
        .video_metadata_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(parse_video_metadata_link)
        .transpose()
        .map_err(|error| result.warnings.push(error))
        .ok()
        .flatten();

    let authored_disc_name = result.source_name.is_some();
    let metadata_capture = report::export_enabled().then(diagnostics::begin_metadata_capture);
    let cache = MetadataCache::platform_default();
    result.content_hash = discdb::content_hash(path, kind);
    let mut discdb_match = None;
    if options.thediscdb_enabled
        && let Some(hash) = result.content_hash.as_deref()
    {
        match discdb::lookup(hash, &cache, cancellation) {
            Ok(Some(found)) => {
                log::debug!(
                    target: "media_rip_engine::metadata_decision",
                    "TheDiscDB lookup result: exact content-hash match; title={:?}; media_type={:?}; year={:?}",
                    found.title,
                    found.media_type,
                    found.year
                );
                result.disc_match_status = Some("Exact TheDiscDB match".to_owned());
                discdb_match = Some(found);
            }
            Ok(None) => {
                log::debug!(
                    target: "media_rip_engine::metadata_decision",
                    "TheDiscDB lookup result: no content-hash match; falling back to title-based providers"
                );
                result.disc_match_status = Some("No exact TheDiscDB record".to_owned());
            }
            Err(error) => {
                log::debug!(
                    target: "media_rip_engine::metadata_decision",
                    "TheDiscDB lookup result: unavailable ({error}); falling back to title-based providers"
                );
                result.disc_match_status = Some("TheDiscDB lookup unavailable".to_owned());
                result
                    .warnings
                    .push(format!("TheDiscDB lookup failed: {error}"));
            }
        }
    } else if options.thediscdb_enabled {
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
                VideoProviderIds {
                    tmdb: matched.tmdb_id,
                    ..Default::default()
                },
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
                    tvdb_id: None,
                    tvmaze_id: None,
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

    let metadata_query = result
        .source_name
        .as_deref()
        .unwrap_or(fallback_name)
        .to_owned();
    let parsed_query = parse_disc_label(&metadata_query);
    let hint = disc_hint_with_link(&parsed_query, metadata_link.as_ref());
    let query_origin = if discdb_match.is_some() {
        "TheDiscDB exact content-hash match title"
    } else if authored_disc_name {
        "authored disc metadata"
    } else if fallback_name == crate::source::source_stem(path) {
        "source path name"
    } else {
        "optical volume label"
    };
    let enabled_providers = metadata_link.as_ref().map_or_else(
        || {
            [
                options
                    .tmdb_access_token
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
                    .then_some("TMDB"),
                options
                    .tvdb_api_key
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty())
                    .then_some("TVDB"),
                options.tvmaze_enabled.then_some("TVMaze"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ")
        },
        |link| link.provider().to_owned(),
    );
    let parsed_hint = hint.map_or_else(
        || "none".to_owned(),
        |hint| {
            format!(
                "season={} volume={} disc={}",
                hint.season,
                hint.volume
                    .map_or_else(|| "none".to_owned(), |value| value.to_string()),
                hint.disc
            )
        },
    );
    log::debug!(
        target: "media_rip_engine::metadata_decision",
        "video metadata lookup plan: input={metadata_query:?}; input_origin={query_origin}; normalized_title={:?}; parsed_disc_hint={parsed_hint}; enabled_providers=[{enabled_providers}]; provider_identity_source={}",
        parsed_query.title,
        if let Some(link) = metadata_link.as_ref() {
            link.provider()
        } else if discdb_match.is_some() {
            "TheDiscDB external IDs or exact matched title"
        } else {
            "normalized_title"
        }
    );
    let (mut tmdb_lookup, mut tvdb_lookup, mut tvmaze_lookup) = parallel_provider_lookups(
        options,
        &cache,
        cancellation,
        &metadata_query,
        hint,
        discdb_match.as_ref(),
        metadata_link.as_ref(),
    );
    let mut previous_fallback_used = false;
    let previous_link = options
        .previous_dvd_match
        .as_ref()
        .and_then(|matched| matched.metadata_url.as_deref())
        .and_then(|url| parse_video_metadata_link(url).ok());
    let previous_hint = options.previous_dvd_match.as_ref().and_then(|matched| {
        Some(DiscHint {
            season: matched.season?,
            volume: None,
            disc: 1,
        })
    });
    if kind == DiscKind::Dvd
        && provider_lookups_empty(&tmdb_lookup, &tvdb_lookup, &tvmaze_lookup)
        && let Some(previous) = options.previous_dvd_match.as_ref()
    {
        log::debug!(
            target: "media_rip_engine::metadata_decision",
            "ordinary DVD identification failed; retrying with previous matched DVD identity {:?}",
            previous.title
        );
        (tmdb_lookup, tvdb_lookup, tvmaze_lookup) = parallel_provider_lookups(
            options,
            &cache,
            cancellation,
            &previous.title,
            previous_hint,
            None,
            previous_link.as_ref(),
        );
        previous_fallback_used =
            !provider_lookups_empty(&tmdb_lookup, &tvdb_lookup, &tvmaze_lookup);
    }
    let provider_lookup_report = [
        provider_lookup_report(
            "TMDB",
            metadata_link.is_none()
                && options
                    .tmdb_access_token
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()),
            &tmdb_lookup,
        ),
        provider_lookup_report(
            "TVDB",
            (metadata_link.is_none()
                && options
                    .tvdb_api_key
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty()))
                || matches!(metadata_link, Some(VideoMetadataLink::Tvdb { .. })),
            &tvdb_lookup,
        ),
        provider_lookup_report(
            "TVMaze",
            (metadata_link.is_none() && options.tvmaze_enabled)
                || matches!(metadata_link, Some(VideoMetadataLink::Tvmaze { .. })),
            &tvmaze_lookup,
        ),
    ];
    let provider_episode_exports = [
        provider_episode_export("TMDB", &tmdb_lookup),
        provider_episode_export("TVDB", &tvdb_lookup),
        provider_episode_export("TVMaze", &tvmaze_lookup),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if options.tvmaze_enabled || matches!(metadata_link, Some(VideoMetadataLink::Tvmaze { .. })) {
        result.tvmaze_match_status = Some(
            match &tvmaze_lookup {
                Ok(Some(lookup)) if lookup.match_method.starts_with("Probable title") => {
                    "TVMaze found a probable match for review"
                }
                Ok(Some(_)) => "TVMaze found an exact match",
                Ok(None) => "TVMaze found no clear match",
                Err(_) => "TVMaze lookup unavailable",
            }
            .to_owned(),
        );
    }
    let (resolved, artwork, mut attributions, warnings) = reconcile_provider_lookups(
        tmdb_lookup,
        tvdb_lookup,
        tvmaze_lookup,
        discdb_match.is_some() || metadata_link.is_some() || previous_fallback_used,
    );
    if previous_fallback_used {
        for attribution in &mut attributions {
            attribution.match_method = "Previous matched DVD identity fallback".to_owned();
        }
    }
    result.warnings.extend(warnings);
    result.attributions.extend(attributions);

    let episodic_context = resolved.as_ref().map_or_else(
        || {
            hint.is_some()
                || discdb_match
                    .as_ref()
                    .is_some_and(|matched| matched.media_type.eq_ignore_ascii_case("Series"))
        },
        |resolved| resolved.matched.media_type == MediaType::Tv,
    );
    let mut spreadsheet_selection = None;
    if let Some(resolved) = resolved {
        if resolved.matched.media_type == MediaType::Tv && !resolved.episodes.is_empty() {
            spreadsheet_selection = Some((
                resolved.matched.title.clone(),
                resolved.matched.providers.clone(),
                resolved.episodes.clone(),
            ));
        }
        result.source_name = Some(resolved.matched.title.clone());
        result.year = resolved.matched.year.or(result.year);
        result.content_type = Some(
            match resolved.matched.media_type {
                MediaType::Movie => "Movie",
                MediaType::Tv => "Series",
            }
            .to_owned(),
        );
        if result.cover_art.is_none() {
            result.cover_art = artwork;
        }
        if discdb_match.is_none() {
            if resolved.matched.media_type == MediaType::Movie {
                apply_movie_match(titles, &resolved.matched);
            } else if let Some(hint) = if previous_fallback_used {
                previous_hint
            } else {
                hint
            } && !resolved.episodes.is_empty()
            {
                let probable = probable_episode_suggestions(
                    titles,
                    &resolved.matched,
                    &resolved.episodes,
                    hint,
                );
                result.suggestions = probable.suggestions;
                result.disc_identity = probable.disc_identity;
                if previous_fallback_used
                    && let Some(first) = options
                        .previous_dvd_match
                        .as_ref()
                        .and_then(|matched| matched.next_episode)
                {
                    set_suggestions_first_episode(&mut result.suggestions, first);
                }
            }
        } else {
            apply_provider_ids(titles, &resolved.matched);
        }
    }
    let lookup_input_report = serde_json::json!({
        "value": metadata_query,
        "origin": query_origin,
        "normalized_title": parsed_query.title,
        "disc_hint": hint.map(|hint| serde_json::json!({
            "season": hint.season,
            "volume": hint.volume,
            "disc": hint.disc,
        })),
    });
    let authored_main_title_count = titles.iter().filter(|title| title.is_main_title).count();
    log::debug!(
        target: "media_rip_engine::metadata_decision",
        "video discovery context: disc_format={kind:?}; resolved_content_type={:?}; authored_main_titles={authored_main_title_count}; episodic_context={episodic_context}",
        result.content_type
    );
    if episodic_context {
        log_episode_matching_report(
            lookup_input_report,
            &provider_lookup_report,
            discdb_match.as_ref(),
            titles,
            &result,
        );
    } else {
        log::debug!(
            target: "media_rip_engine::metadata_decision",
            "episode matching diagnostics skipped: no series match or season/disc hint was established"
        );
    }
    if let Some(metadata_capture) = metadata_capture {
        let raw_payloads = metadata_capture.finish();
        if let Some((selected_show, providers, episodes)) = spreadsheet_selection {
            report::export_episode_alignment(report::EpisodeAlignmentExport {
                selected_show: &selected_show,
                season: hint.map(|hint| hint.season),
                providers: &providers,
                episodes: &episodes,
                provider_episodes: &provider_episode_exports,
                titles,
                suggestions: &result.suggestions,
                raw_payloads: &raw_payloads,
            });
        } else if report::export_enabled() {
            log::debug!(
                target: "media_rip_engine::metadata_decision",
                "episode alignment export skipped: no TV series with episode data was selected"
            );
        }
    }
    result
}

fn provider_lookups_empty(
    tmdb: &ProviderLookupResult,
    tvdb: &ProviderLookupResult,
    tvmaze: &ProviderLookupResult,
) -> bool {
    [tmdb, tvdb, tvmaze]
        .into_iter()
        .all(|lookup| !matches!(lookup, Ok(Some(_))))
}

fn set_suggestions_first_episode(suggestions: &mut [VideoMetadataSuggestion], first: u32) {
    let Some(current) = suggestions
        .iter()
        .filter_map(|suggestion| suggestion.metadata.episode)
        .min()
    else {
        return;
    };
    let Ok(offset) = i32::try_from(i64::from(first) - i64::from(current)) else {
        return;
    };
    let replacements = suggestions
        .iter()
        .map(|suggestion| {
            let episode = suggestion.metadata.episode?.checked_add_signed(offset)?;
            let choice = suggestion
                .episode_choices
                .iter()
                .find(|choice| choice.episode == episode)?;
            Some((episode, choice.name.clone(), choice.tags.clone()))
        })
        .collect::<Option<Vec<_>>>();
    if let Some(replacements) = replacements {
        for (suggestion, (episode, name, tags)) in suggestions.iter_mut().zip(replacements) {
            suggestion.metadata.episode = Some(episode);
            suggestion.name = name;
            suggestion.tags = tags;
            suggestion.metadata.attribution.match_method =
                "Previous matched DVD identity + continued episode sequence".to_owned();
        }
    }
}

#[derive(Debug)]
struct ProviderLookup {
    match_method: &'static str,
    matched: VideoMatch,
    episodes: Vec<VideoEpisode>,
    artwork: Option<CoverArt>,
    warnings: Vec<String>,
}

type ProviderLookupResult = Result<Option<ProviderLookup>, String>;

fn provider_lookup_report(
    provider: &str,
    enabled: bool,
    lookup: &ProviderLookupResult,
) -> serde_json::Value {
    if !enabled {
        return serde_json::json!({
            "provider": provider,
            "status": "disabled",
        });
    }
    match lookup {
        Err(error) => serde_json::json!({
            "provider": provider,
            "status": "error",
            "error": error,
        }),
        Ok(None) => serde_json::json!({
            "provider": provider,
            "status": "no_selected_match",
            "note": "Raw API candidates, including rejected or ambiguous results, remain available under the media_rip_engine::metadata_payload log target.",
        }),
        Ok(Some(lookup)) => serde_json::json!({
            "provider": provider,
            "status": "selected_match",
            "match_method": lookup.match_method,
            "matched_title": lookup.matched.title,
            "media_type": match lookup.matched.media_type {
                MediaType::Movie => "movie",
                MediaType::Tv => "series",
            },
            "year": lookup.matched.year,
            "runtime_minutes": lookup.matched.runtime_minutes,
            "provider_ids": {
                "tmdb": lookup.matched.tmdb_id,
                "tvdb": lookup.matched.tvdb_id,
                "tvmaze": lookup.matched.tvmaze_id,
            },
            "season_episodes_returned": lookup.episodes.iter().map(|episode| serde_json::json!({
                "episode": episode.episode_number,
                "name": episode.name,
                "runtime_minutes": episode.runtime,
            })).collect::<Vec<_>>(),
            "warnings": lookup.warnings,
        }),
    }
}

fn provider_episode_export(
    provider: &str,
    lookup: &ProviderLookupResult,
) -> Option<report::ProviderEpisodeExport> {
    let lookup = lookup.as_ref().ok()?.as_ref()?;
    Some(report::ProviderEpisodeExport {
        provider: provider.to_owned(),
        selected_show: lookup.matched.title.clone(),
        episodes: lookup.episodes.clone(),
    })
}

fn log_episode_matching_report(
    lookup_input: serde_json::Value,
    provider_lookups: &[serde_json::Value],
    discdb_match: Option<&discdb::DiscDbMatch>,
    titles: &[Title],
    result: &VideoEnrichment,
) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let title_reports = titles
        .iter()
        .map(|title| disc_title_report(title, &result.suggestions))
        .collect::<Vec<_>>();
    let mut available_episode_choices = Vec::new();
    for choice in result
        .suggestions
        .iter()
        .flat_map(|suggestion| &suggestion.episode_choices)
    {
        if !available_episode_choices
            .iter()
            .any(|existing: &&VideoEpisodeChoice| existing.episode == choice.episode)
        {
            available_episode_choices.push(choice);
        }
    }
    let applied_count = titles
        .iter()
        .filter(|title| title.video_metadata.is_some())
        .count();
    let suggested_count = titles
        .iter()
        .filter(|title| {
            title.video_metadata.is_none()
                && result
                    .suggestions
                    .iter()
                    .any(|suggestion| suggestion.title_id == title.id)
        })
        .count();
    let report = serde_json::json!({
        "schema": "media-rip.episode-matching-report.v1",
        "purpose": "Correlate API episode data with every disc title for manual matching checks.",
        "raw_api_payload_log_target": "media_rip_engine::metadata_payload",
        "lookup_input": lookup_input,
        "thediscdb": {
            "content_hash": result.content_hash,
            "status": result.disc_match_status,
            "matched_identity": discdb_match.map(|matched| serde_json::json!({
                "title": matched.title,
                "media_type": matched.media_type,
                "year": matched.year,
                "tmdb_id": matched.tmdb_id,
                "imdb_id": matched.imdb_id,
                "disc_slug": matched.disc_slug,
                "title_mappings": matched.titles,
            })),
        },
        "provider_lookups": provider_lookups,
        "outcome_summary": {
            "disc_title_count": titles.len(),
            "applied_count": applied_count,
            "suggested_count": suggested_count,
            "unmatched_count": titles.len().saturating_sub(applied_count + suggested_count),
        },
        "available_episode_choices": available_episode_choices,
        "disc_titles": title_reports,
        "final_enrichment": {
            "resolved_source_name": result.source_name,
            "year": result.year,
            "content_type": result.content_type,
            "disc_identity": result.disc_identity,
            "attributions": result.attributions,
            "warnings": result.warnings,
        },
    });
    diagnostics::episode_matching_report(&report);
}

fn disc_title_report(title: &Title, suggestions: &[VideoMetadataSuggestion]) -> serde_json::Value {
    let suggestion = suggestions
        .iter()
        .find(|suggestion| suggestion.title_id == title.id);
    let status = if title.video_metadata.is_some() {
        "applied"
    } else if suggestion.is_some() {
        "suggested_for_review"
    } else {
        "unmatched"
    };
    serde_json::json!({
        "match_status": status,
        "disc_title": title,
        "suggestion": suggestion.map(|suggestion| serde_json::json!({
            "name": suggestion.name,
            "tags": suggestion.tags,
            "metadata": suggestion.metadata,
            "evidence": suggestion.evidence,
            "assumptions": suggestion.assumptions,
        })),
    })
}

fn parallel_provider_lookups(
    options: &ScanOptions,
    cache: &MetadataCache,
    cancellation: &CancellationToken,
    metadata_query: &str,
    hint: Option<DiscHint>,
    discdb_match: Option<&discdb::DiscDbMatch>,
    metadata_link: Option<&VideoMetadataLink>,
) -> (
    ProviderLookupResult,
    ProviderLookupResult,
    ProviderLookupResult,
) {
    let tmdb_token = options
        .tmdb_access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let tvdb_key = options
        .tvdb_api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let tvdb_pin = options
        .tvdb_subscriber_pin
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    std::thread::scope(|scope| {
        let tmdb_handle = metadata_link
            .is_none()
            .then_some(tmdb_token)
            .flatten()
            .map(|token| {
                scope.spawn(move || {
                    lookup_tmdb(
                        token,
                        cache,
                        cancellation,
                        metadata_query,
                        hint,
                        discdb_match,
                    )
                })
            });
        let tvdb_handle = tvdb_key
            .filter(|_| {
                metadata_link.is_none()
                    || matches!(metadata_link, Some(VideoMetadataLink::Tvdb { .. }))
            })
            .map(|key| {
                scope.spawn(move || {
                    lookup_tvdb(
                        (key, tvdb_pin),
                        cache,
                        cancellation,
                        metadata_query,
                        hint,
                        discdb_match,
                        metadata_link,
                    )
                })
            });
        let tvmaze_handle = (matches!(metadata_link, Some(VideoMetadataLink::Tvmaze { .. }))
            || (metadata_link.is_none() && options.tvmaze_enabled))
            .then(|| {
                scope.spawn(move || {
                    lookup_tvmaze(
                        cache,
                        cancellation,
                        metadata_query,
                        hint,
                        discdb_match,
                        metadata_link,
                    )
                })
            });
        let tmdb = tmdb_handle.map_or(Ok(None), |handle| {
            handle
                .join()
                .unwrap_or_else(|_| Err("TMDB lookup worker panicked".to_owned()))
        });
        let tvdb = if matches!(metadata_link, Some(VideoMetadataLink::Tvdb { .. }))
            && tvdb_key.is_none()
        {
            Err("A TVDB API key is required to use a TheTVDB metadata link.".to_owned())
        } else {
            tvdb_handle.map_or(Ok(None), |handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("TVDB lookup worker panicked".to_owned()))
            })
        };
        let tvmaze = tvmaze_handle.map_or(Ok(None), |handle| {
            handle
                .join()
                .unwrap_or_else(|_| Err("TVMaze lookup worker panicked".to_owned()))
        });
        (tmdb, tvdb, tvmaze)
    })
}

fn lookup_tmdb(
    token: &str,
    cache: &MetadataCache,
    cancellation: &CancellationToken,
    metadata_query: &str,
    hint: Option<DiscHint>,
    discdb_match: Option<&discdb::DiscDbMatch>,
) -> Result<Option<ProviderLookup>, String> {
    let client = TmdbClient::new(token, cache, cancellation);
    let mut episodes = Vec::new();
    let mut exact_review_title = true;
    let matched = if let Some(disc) = discdb_match {
        let Some(id) = disc.tmdb_id else {
            return Ok(None);
        };
        let media_type = if disc.media_type.eq_ignore_ascii_case("Series") {
            MediaType::Tv
        } else {
            MediaType::Movie
        };
        client.details(id, media_type)?
    } else {
        let query = searchable_disc_name(metadata_query);
        let found = if let Some(hint) = hint {
            client.search_tv_season_review(&query, hint.season)?.map(
                |(matched, found_episodes, exact_title)| {
                    exact_review_title = exact_title;
                    episodes = found_episodes
                        .into_iter()
                        .map(|episode| VideoEpisode {
                            episode_number: episode.episode_number,
                            name: episode.name,
                            runtime: episode.runtime,
                        })
                        .collect();
                    matched
                },
            )
        } else {
            client.search_exact(&query)?
        };
        let Some(found) = found else {
            return Ok(None);
        };
        found
    };
    let mut warnings = Vec::new();
    let artwork = matched.poster_path.as_deref().and_then(|path| {
        client
            .artwork(path)
            .map_err(|error| {
                warnings.push(format!("TMDB artwork failed: {error}"));
            })
            .ok()
    });
    Ok(Some(ProviderLookup {
        match_method: if discdb_match.is_some() {
            "TheDiscDB external ID"
        } else if hint.is_some() && exact_review_title {
            "Exact normalized title + authored season"
        } else if hint.is_some() {
            "Probable title + authored season review"
        } else {
            "Exact normalized title match"
        },
        matched: matched.into(),
        episodes,
        artwork,
        warnings,
    }))
}

fn lookup_tvdb(
    credentials: (&str, Option<&str>),
    cache: &MetadataCache,
    cancellation: &CancellationToken,
    metadata_query: &str,
    hint: Option<DiscHint>,
    discdb_match: Option<&discdb::DiscDbMatch>,
    metadata_link: Option<&VideoMetadataLink>,
) -> Result<Option<ProviderLookup>, String> {
    let (api_key, subscriber_pin) = credentials;
    let client = TvdbClient::new(api_key, subscriber_pin, cache, cancellation);
    let mut episodes = Vec::new();
    let mut exact_review_title = true;
    let matched = if let Some(VideoMetadataLink::Tvdb {
        media_type, slug, ..
    }) = metadata_link
    {
        client.lookup_slug(slug, *media_type)?
    } else if metadata_link.is_some() {
        return Ok(None);
    } else if let Some(disc) = discdb_match {
        let media_type = if disc.media_type.eq_ignore_ascii_case("Series") {
            TvdbMediaType::Tv
        } else {
            TvdbMediaType::Movie
        };
        let found = if let Some(imdb_id) = disc.imdb_id.as_deref() {
            client.search_remote(imdb_id, media_type)?
        } else {
            client.search_exact_type(&disc.title, media_type)?
        };
        let Some(found) = found else {
            return Ok(None);
        };
        found
    } else {
        let query = searchable_disc_name(metadata_query);
        let found = if let Some(hint) = hint {
            client.search_tv_season_review(&query, hint.season)?.map(
                |(matched, found_episodes, exact_title)| {
                    exact_review_title = exact_title;
                    episodes = found_episodes
                        .into_iter()
                        .map(|episode| VideoEpisode {
                            episode_number: episode.episode_number,
                            name: episode.name,
                            runtime: episode.runtime,
                        })
                        .collect();
                    matched
                },
            )
        } else {
            client.search_exact(&query)?
        };
        let Some(found) = found else {
            return Ok(None);
        };
        found
    };
    if episodes.is_empty()
        && matched.media_type == TvdbMediaType::Tv
        && let Some(hint) = hint
    {
        episodes = client
            .season(matched.id, hint.season)?
            .into_iter()
            .map(|episode| VideoEpisode {
                episode_number: episode.episode_number,
                name: episode.name,
                runtime: episode.runtime,
            })
            .collect();
    }
    let mut warnings = Vec::new();
    let artwork = matched.artwork_url.as_deref().and_then(|url| {
        client
            .artwork(url)
            .map_err(|error| {
                warnings.push(format!("TVDB artwork failed: {error}"));
            })
            .ok()
    });
    Ok(Some(ProviderLookup {
        match_method: if metadata_link.is_some() {
            "User-provided TheTVDB URL"
        } else if discdb_match
            .and_then(|disc| disc.imdb_id.as_ref())
            .is_some()
        {
            "TheDiscDB IMDb external ID"
        } else if discdb_match.is_some() {
            "TheDiscDB identity + exact normalized title"
        } else if hint.is_some() && exact_review_title {
            "Exact normalized title + authored season + DVD order"
        } else if hint.is_some() {
            "Probable title + authored season review + DVD order"
        } else {
            "Exact normalized title match"
        },
        matched: matched.into(),
        episodes,
        artwork,
        warnings,
    }))
}

fn lookup_tvmaze(
    cache: &MetadataCache,
    cancellation: &CancellationToken,
    metadata_query: &str,
    hint: Option<DiscHint>,
    discdb_match: Option<&discdb::DiscDbMatch>,
    metadata_link: Option<&VideoMetadataLink>,
) -> Result<Option<ProviderLookup>, String> {
    if discdb_match.is_some_and(|disc| !disc.media_type.eq_ignore_ascii_case("Series")) {
        return Ok(None);
    }
    let client = TvmazeClient::new(cache, cancellation);
    let mut episodes = Vec::new();
    let mut exact_review_title = true;
    let matched = if let Some(VideoMetadataLink::Tvmaze { show_id }) = metadata_link {
        client.show(*show_id)?
    } else if metadata_link.is_some() {
        return Ok(None);
    } else if let Some(disc) = discdb_match {
        let found = if let Some(imdb_id) = disc.imdb_id.as_deref() {
            client.lookup_imdb(imdb_id)?
        } else {
            client.search_exact(&disc.title)?
        };
        let Some(found) = found else {
            return Ok(None);
        };
        found
    } else {
        let query = searchable_disc_name(metadata_query);
        let found = if let Some(hint) = hint {
            client.search_tv_season_review(&query, hint.season)?.map(
                |(matched, found_episodes, exact_title)| {
                    exact_review_title = exact_title;
                    episodes = found_episodes
                        .into_iter()
                        .map(|episode| VideoEpisode {
                            episode_number: episode.episode_number,
                            name: episode.name,
                            runtime: episode.runtime,
                        })
                        .collect();
                    matched
                },
            )
        } else {
            client.search_exact(&query)?
        };
        let Some(found) = found else {
            return Ok(None);
        };
        found
    };
    if episodes.is_empty()
        && let Some(hint) = hint
    {
        episodes = client
            .season(matched.id, hint.season)?
            .into_iter()
            .map(|episode| VideoEpisode {
                episode_number: episode.episode_number,
                name: episode.name,
                runtime: episode.runtime,
            })
            .collect();
    }
    let mut warnings = Vec::new();
    let artwork = matched.artwork_url.as_deref().and_then(|url| {
        client
            .artwork(url)
            .map_err(|error| warnings.push(format!("TVMaze artwork failed: {error}")))
            .ok()
    });
    Ok(Some(ProviderLookup {
        match_method: if metadata_link.is_some() {
            "User-provided TVMaze URL"
        } else if discdb_match
            .and_then(|disc| disc.imdb_id.as_ref())
            .is_some()
        {
            "TheDiscDB IMDb external ID"
        } else if discdb_match.is_some() {
            "TheDiscDB identity + exact normalized title"
        } else if hint.is_some() && exact_review_title {
            "Exact normalized title + authored season"
        } else if hint.is_some() {
            "Probable title + authored season review"
        } else {
            "Exact normalized title match"
        },
        matched: matched.into(),
        episodes,
        artwork,
        warnings,
    }))
}

fn reconcile_provider_lookups(
    tmdb: Result<Option<ProviderLookup>, String>,
    tvdb: Result<Option<ProviderLookup>, String>,
    tvmaze: Result<Option<ProviderLookup>, String>,
    exact_disc_match: bool,
) -> (
    Option<ProviderLookup>,
    Option<CoverArt>,
    Vec<MetadataAttribution>,
    Vec<String>,
) {
    let mut warnings = Vec::new();
    let tmdb = match tmdb {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("TMDB lookup failed: {error}"));
            None
        }
    };
    let tvdb = match tvdb {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("TVDB lookup failed: {error}"));
            None
        }
    };
    let tvmaze = match tvmaze {
        Ok(value) => value,
        Err(error) => {
            warnings.push(format!("TVMaze lookup failed: {error}"));
            None
        }
    };

    let mut candidates = [tmdb, tvdb, tvmaze]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return (None, None, Vec::new(), warnings);
    }
    let agreement_counts = candidates
        .iter()
        .map(|candidate| {
            candidates
                .iter()
                .filter(|other| provider_matches_agree(&candidate.matched, &other.matched))
                .count()
        })
        .collect::<Vec<_>>();
    let best_count = agreement_counts.iter().copied().max().unwrap_or(1);
    if candidates.len() > 1 && best_count == 1 && !exact_disc_match {
        warnings.push(format!(
            "Video metadata providers identified conflicting titles ({}); no title-only provider result was applied",
            candidates
                .iter()
                .map(|candidate| format!(
                    "{}: {:?}",
                    candidate.matched.providers.join(" + "),
                    candidate.matched.title
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for candidate in candidates {
            warnings.extend(candidate.warnings);
        }
        return (None, None, Vec::new(), warnings);
    }
    let selected_index = agreement_counts
        .iter()
        .position(|count| *count == best_count)
        .unwrap_or(0);
    let mut selected = candidates.remove(selected_index);
    for mut candidate in candidates {
        if provider_matches_agree(&selected.matched, &candidate.matched) {
            selected = merge_provider_lookups(selected, candidate);
        } else {
            warnings.push(format!(
                "{} identified {:?} while {} identified {:?}; conflicting provider metadata was not combined",
                selected.matched.providers.join(" + "),
                selected.matched.title,
                candidate.matched.providers.join(" + "),
                candidate.matched.title
            ));
            warnings.append(&mut candidate.warnings);
        }
    }
    warnings.append(&mut selected.warnings);
    let confidence = if exact_disc_match {
        98
    } else if selected.matched.providers.len() > 1 {
        88
    } else {
        75
    };
    let attributions = selected
        .matched
        .providers
        .iter()
        .map(|provider| {
            MetadataAttribution::new(
                *provider,
                confidence,
                if selected.matched.providers.len() > 1 {
                    "Independent provider agreement"
                } else {
                    selected.match_method
                },
            )
        })
        .collect();
    let artwork = selected.artwork.take();
    (Some(selected), artwork, attributions, warnings)
}

fn provider_matches_agree(first: &VideoMatch, second: &VideoMatch) -> bool {
    if first.media_type != second.media_type {
        return false;
    }
    let titles_agree = tmdb::normalize_title(&first.title) == tmdb::normalize_title(&second.title);
    match (first.year, second.year) {
        (Some(first), Some(second)) => first == second,
        _ => titles_agree,
    }
}

fn merge_provider_lookups(
    mut selected: ProviderLookup,
    incoming: ProviderLookup,
) -> ProviderLookup {
    selected
        .matched
        .providers
        .extend(incoming.matched.providers.iter().copied());
    selected.matched.tmdb_id = selected.matched.tmdb_id.or(incoming.matched.tmdb_id);
    selected.matched.tvdb_id = selected.matched.tvdb_id.or(incoming.matched.tvdb_id);
    selected.matched.tvmaze_id = selected.matched.tvmaze_id.or(incoming.matched.tvmaze_id);
    selected.matched.year = selected.matched.year.or(incoming.matched.year);
    selected.matched.runtime_minutes = selected
        .matched
        .runtime_minutes
        .or(incoming.matched.runtime_minutes);
    selected.episodes = if incoming.matched.providers.contains(&"TVDB") {
        merge_episode_metadata(selected.episodes, incoming.episodes)
    } else {
        merge_episode_metadata(incoming.episodes, selected.episodes)
    };
    selected.artwork = selected.artwork.or(incoming.artwork);
    selected.warnings.extend(incoming.warnings);
    selected.match_method = "Independent provider agreement";
    selected
}

fn merge_episode_metadata(
    tmdb_episodes: Vec<VideoEpisode>,
    mut tvdb_episodes: Vec<VideoEpisode>,
) -> Vec<VideoEpisode> {
    if tvdb_episodes.is_empty() {
        return tmdb_episodes;
    }
    for episode in &mut tvdb_episodes {
        let Some(tmdb_episode) = tmdb_episodes
            .iter()
            .find(|candidate| candidate.episode_number == episode.episode_number)
        else {
            continue;
        };
        let names_agree =
            tmdb::normalize_title(&episode.name) == tmdb::normalize_title(&tmdb_episode.name);
        if episode.runtime.is_none() && names_agree {
            episode.runtime = tmdb_episode.runtime;
        }
        if episode.name.trim().is_empty() {
            episode.name.clone_from(&tmdb_episode.name);
        }
    }
    tvdb_episodes
}

fn apply_provider_ids(titles: &mut [Title], matched: &VideoMatch) {
    for title in titles
        .iter_mut()
        .filter(|title| title.video_metadata.is_some())
    {
        if let Some(metadata) = &mut title.video_metadata {
            metadata.tmdb_id = matched.tmdb_id.or(metadata.tmdb_id);
            metadata.tvdb_id = matched.tvdb_id.or(metadata.tvdb_id);
            metadata.tvmaze_id = matched.tvmaze_id.or(metadata.tvmaze_id);
            metadata.year = metadata.year.or(matched.year);
        }
        if let Some(id) = matched.tmdb_id {
            set_tag(&mut title.tags, "tmdb_id", &id.to_string());
        }
        if let Some(id) = matched.tvdb_id {
            set_tag(&mut title.tags, "tvdb_id", &id.to_string());
        }
        if let Some(id) = matched.tvmaze_id {
            set_tag(&mut title.tags, "tvmaze_id", &id.to_string());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiscHint {
    season: u32,
    volume: Option<u32>,
    disc: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscLabelKind {
    Season,
    Volume,
    Disc,
}

// Keep aliases longest-first so compact labels such as `season3` are matched
// before their one-letter forms. Adding a new spelling here makes it available
// to both disc-hint extraction and metadata-search normalization.
const DISC_LABEL_ALIASES: &[(&str, DiscLabelKind)] = &[
    ("season", DiscLabelKind::Season),
    ("volume", DiscLabelKind::Volume),
    ("disk", DiscLabelKind::Disc),
    ("disc", DiscLabelKind::Disc),
    ("vol", DiscLabelKind::Volume),
    ("s", DiscLabelKind::Season),
    ("v", DiscLabelKind::Volume),
    ("d", DiscLabelKind::Disc),
];

const DISC_MEDIA_SUFFIXES: &[&str] = &["dvd", "bluray", "bd"];

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedDiscLabel {
    title: String,
    season: Option<u32>,
    volume: Option<u32>,
    disc: Option<u32>,
}

impl ParsedDiscLabel {
    fn hint(&self) -> Option<DiscHint> {
        Some(DiscHint {
            season: self.season?,
            volume: self.volume,
            disc: self.disc?,
        })
    }
}

fn disc_hint_with_link(
    parsed: &ParsedDiscLabel,
    metadata_link: Option<&VideoMetadataLink>,
) -> Option<DiscHint> {
    parsed.hint().or_else(|| {
        let metadata_link = metadata_link?;
        Some(DiscHint {
            season: metadata_link.season().or_else(|| {
                metadata_link
                    .is_series()
                    .then(|| trailing_compact_season(&parsed.title))
                    .flatten()
            })?,
            volume: parsed.volume,
            disc: parsed.disc?,
        })
    })
}

fn trailing_compact_season(value: &str) -> Option<u32> {
    let marker = value.rfind(['s', 'S'])?;
    let number = value.get(marker + 1..)?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| number.parse::<u32>().ok())
        .flatten()
        .filter(|season| *season > 0)
}

fn label_kind(value: &str) -> Option<DiscLabelKind> {
    DISC_LABEL_ALIASES
        .iter()
        .find_map(|(alias, kind)| value.eq_ignore_ascii_case(alias).then_some(*kind))
}

fn compact_disc_components(value: &str) -> Option<Vec<(DiscLabelKind, u32)>> {
    let normalized = value.to_ascii_lowercase();
    let mut remainder = normalized.as_str();
    let mut components = Vec::new();
    while !remainder.is_empty() {
        let (kind, rest) = DISC_LABEL_ALIASES
            .iter()
            .find_map(|(alias, kind)| remainder.strip_prefix(alias).map(|rest| (*kind, rest)))?;
        let digit_count = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digit_count == 0 {
            return None;
        }
        let number = rest[..digit_count]
            .parse()
            .ok()
            .filter(|number| *number > 0)?;
        components.push((kind, number));
        remainder = &rest[digit_count..];
    }
    Some(components)
}

fn attached_disc_components(value: &str) -> Option<(&str, Vec<(DiscLabelKind, u32)>)> {
    const ATTACHED_ALIASES: &[&str] = &["season", "series", "disc", "disk", "volume", "vol"];
    value.char_indices().skip(1).find_map(|(index, _)| {
        let suffix = &value[index..];
        ATTACHED_ALIASES
            .iter()
            .any(|alias| suffix.to_ascii_lowercase().starts_with(alias))
            .then(|| compact_disc_components(suffix))
            .flatten()
            .map(|components| (&value[..index], components))
    })
}

fn parse_disc_label(value: &str) -> ParsedDiscLabel {
    let parts = value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut parsed = ParsedDiscLabel::default();
    let mut attached_title_suffix = None;
    let mut suffix_start = parts.len();
    while suffix_start > 0 {
        let last = parts[suffix_start - 1];
        if DISC_MEDIA_SUFFIXES
            .iter()
            .any(|suffix| last.eq_ignore_ascii_case(suffix))
        {
            suffix_start -= 1;
            continue;
        }
        if suffix_start >= 2
            && parts[suffix_start - 2].eq_ignore_ascii_case("blu")
            && last.eq_ignore_ascii_case("ray")
        {
            suffix_start -= 2;
            continue;
        }
        let components = compact_disc_components(last)
            .map(|components| (components, 1, None))
            .or_else(|| {
                attached_disc_components(last)
                    .map(|(title, components)| (components, 1, Some(title)))
            })
            .or_else(|| {
                let number = last.parse().ok().filter(|number| *number > 0)?;
                let kind = label_kind(parts.get(suffix_start.checked_sub(2)?)?)?;
                Some((vec![(kind, number)], 2, None))
            });
        let Some((components, consumed, title_suffix)) = components else {
            break;
        };
        attached_title_suffix = title_suffix.or(attached_title_suffix);
        // Work from the end of the label, retaining the closest occurrence if
        // malformed media contains the same component more than once.
        for (kind, number) in components.into_iter().rev() {
            match kind {
                DiscLabelKind::Season => parsed.season.get_or_insert(number),
                DiscLabelKind::Volume => parsed.volume.get_or_insert(number),
                DiscLabelKind::Disc => parsed.disc.get_or_insert(number),
            };
        }
        suffix_start -= consumed;
    }
    parsed.title = parts[..suffix_start]
        .iter()
        .copied()
        .chain(attached_title_suffix)
        .collect::<Vec<_>>()
        .join(" ");
    parsed
}

#[cfg(test)]
fn disc_hint(value: &str) -> Option<DiscHint> {
    parse_disc_label(value).hint()
}

#[derive(Debug, Default)]
struct ProbableSuggestions {
    suggestions: Vec<VideoMetadataSuggestion>,
    disc_identity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EpisodeRange {
    first: u32,
    count: u32,
    overall_disc: u32,
    estimated_total_discs: u32,
    estimated_discs_in_volume: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeAlignment {
    first: u32,
    runtime_error_ms: u64,
    runner_up_first: Option<u32>,
    runner_up_score_ms: Option<u64>,
    compared_episodes: usize,
    scale_per_mille: u64,
}

fn runtime_alignment(
    titles: &[Title],
    group: &[usize],
    episodes: &[VideoEpisode],
    estimated_first: u32,
) -> Option<RuntimeAlignment> {
    const POSITION_PENALTY_MS: u64 = 12_000;
    const MIN_WIN_MARGIN_MS: u64 = 20_000;
    const MIN_RUNTIME_EPISODES: usize = 2;

    if group.len() < MIN_RUNTIME_EPISODES || episodes.len() < group.len() {
        return None;
    }
    let mut candidates = Vec::new();
    for window in episodes.windows(group.len()) {
        if !window
            .windows(2)
            .all(|pair| pair[1].episode_number == pair[0].episode_number + 1)
        {
            continue;
        }
        let pairs = group
            .iter()
            .zip(window)
            .filter_map(|(title_index, episode)| {
                let expected_ms = episode.runtime?.saturating_mul(60_000);
                (expected_ms > 0).then_some((titles[*title_index].duration_ms, expected_ms))
            })
            .collect::<Vec<_>>();
        if pairs.len() < MIN_RUNTIME_EPISODES {
            continue;
        }

        // Account for a consistent edition offset or PAL speed-up without
        // allowing runtime matching to explain away materially different cuts.
        let actual_total = pairs.iter().map(|(actual, _)| *actual).sum::<u64>();
        let expected_total = pairs.iter().map(|(_, expected)| *expected).sum::<u64>();
        let scale_per_mille = actual_total
            .saturating_mul(1_000)
            .checked_div(expected_total)?
            .clamp(940, 1_060);
        let runtime_error = pairs
            .iter()
            .map(|(actual, expected)| {
                actual.abs_diff(expected.saturating_mul(scale_per_mille) / 1_000)
            })
            .sum::<u64>()
            / pairs.len() as u64;
        let first = window[0].episode_number;
        let position_error =
            u64::from(first.abs_diff(estimated_first)).saturating_mul(POSITION_PENALTY_MS);
        candidates.push((
            runtime_error.saturating_add(position_error),
            first,
            pairs.len(),
            scale_per_mille,
            runtime_error,
        ));
    }
    candidates.sort_unstable();
    let (score_ms, first, compared_episodes, scale_per_mille, runtime_error_ms) =
        *candidates.first()?;
    let runner_up = candidates.get(1).copied();
    let decisive =
        runner_up.is_none_or(|runner| runner.0 >= score_ms.saturating_add(MIN_WIN_MARGIN_MS));
    if !decisive {
        return None;
    }
    Some(RuntimeAlignment {
        first,
        runtime_error_ms,
        runner_up_first: runner_up.map(|candidate| candidate.1),
        runner_up_score_ms: runner_up.map(|candidate| candidate.0),
        compared_episodes,
        scale_per_mille,
    })
}

fn balanced_partition(total: u32, partitions: u32, partition: u32) -> Option<(u32, u32)> {
    if total == 0 || partitions == 0 || partition == 0 || partition > partitions {
        return None;
    }
    let base = total / partitions;
    let larger_partitions = total % partitions;
    let preceding = partition - 1;
    let count = base + u32::from(partition <= larger_partitions);
    let first = preceding * base + preceding.min(larger_partitions) + 1;
    Some((first, count))
}

fn inferred_episode_range(
    episode_count: u32,
    episodes_per_disc: u32,
    group_size: u32,
    hint: DiscHint,
) -> Option<EpisodeRange> {
    let estimated_total_discs = episode_count.div_ceil(episodes_per_disc).max(1);
    if let Some(volume) = hint.volume {
        // A label such as VOL2 identifies the ordinal, but not the number of
        // volumes in the release. For a useful review suggestion we treat the
        // current volume as the final one, and expose that assumption in the UI.
        let assumed_total_volumes = volume;
        let (volume_first_episode, volume_episode_count) =
            balanced_partition(episode_count, assumed_total_volumes, volume)?;
        let (volume_first_disc, discs_in_volume) =
            balanced_partition(estimated_total_discs, assumed_total_volumes, volume)?;
        if hint.disc > discs_in_volume {
            return None;
        }
        let (first_in_volume, expected_count) =
            balanced_partition(volume_episode_count, discs_in_volume, hint.disc)?;
        return Some(EpisodeRange {
            first: volume_first_episode + first_in_volume - 1,
            count: expected_count.min(group_size.max(1)),
            overall_disc: volume_first_disc + hint.disc - 1,
            estimated_total_discs,
            estimated_discs_in_volume: Some(discs_in_volume),
        });
    }

    let overall_disc = hint.disc.min(estimated_total_discs);
    let (first, expected_count) =
        balanced_partition(episode_count, estimated_total_discs, overall_disc)?;
    Some(EpisodeRange {
        first,
        count: expected_count.min(group_size.max(1)),
        overall_disc,
        estimated_total_discs,
        estimated_discs_in_volume: None,
    })
}

fn matching_stream_layout(titles: &[Title], group: &[usize]) -> bool {
    let signature = |title: &Title| {
        let mut tracks = title
            .tracks
            .iter()
            .filter(|track| {
                matches!(
                    track.kind,
                    media_rip_core::StreamKind::Audio | media_rip_core::StreamKind::Subtitle
                )
            })
            .map(|track| {
                let stable_properties = track
                    .properties
                    .iter()
                    // Per-stream durations differ between otherwise identical
                    // episode layouts and are already compared separately.
                    .filter(|property| !property.contains(':'))
                    .cloned()
                    .collect::<Vec<_>>();
                (
                    match track.kind {
                        media_rip_core::StreamKind::Audio => "audio",
                        media_rip_core::StreamKind::Subtitle => "subtitle",
                        _ => unreachable!(),
                    },
                    track.codec.clone(),
                    track.language.clone(),
                    stable_properties,
                )
            })
            .collect::<Vec<_>>();
        tracks.sort_unstable();
        tracks
    };

    let Some(first) = group.first().map(|index| signature(&titles[*index])) else {
        return false;
    };
    !first.is_empty()
        && group
            .iter()
            .skip(1)
            .all(|index| signature(&titles[*index]) == first)
}

fn probable_episode_suggestions(
    titles: &[Title],
    series: &VideoMatch,
    episodes: &[VideoEpisode],
    hint: DiscHint,
) -> ProbableSuggestions {
    const MIN_EPISODE_MS: u64 = 20 * 60_000;
    const DEFAULT_MAX_EPISODE_MS: u64 = 75 * 60_000;
    const ABSOLUTE_MAX_EPISODE_MS: u64 = 120 * 60_000;
    let max_episode_ms = episodes
        .iter()
        .filter_map(|episode| episode.runtime)
        .max()
        .map(|minutes| minutes.saturating_add(8).saturating_mul(60_000))
        .unwrap_or(DEFAULT_MAX_EPISODE_MS)
        .clamp(DEFAULT_MAX_EPISODE_MS, ABSOLUTE_MAX_EPISODE_MS);
    let episode_like = |title: &Title| {
        (MIN_EPISODE_MS..=max_episode_ms).contains(&title.duration_ms) && title.chapters.len() >= 2
    };
    let mut groups = Vec::<Vec<usize>>::new();
    for (play_all_index, play_all) in titles.iter().enumerate() {
        if play_all.duration_ms <= max_episode_ms {
            continue;
        }
        let mut group = Vec::new();
        let mut total = 0_u64;
        for (index, title) in titles.iter().enumerate().skip(play_all_index + 1) {
            if title.duration_ms > max_episode_ms {
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
    let has_authored_play_all_group = !groups.is_empty();
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
    let episode_count = episodes.len() as u32;
    let group_size = groups.first().map_or(0, Vec::len) as u32;
    let fallback_episodes_per_disc = (180 * 60_000 / median_runtime).clamp(2, 8) as u32;
    let episodes_per_disc = if has_authored_play_all_group {
        // A short authored group may be the partially filled final disc. It
        // proves how many titles are on this disc, but not the package's usual
        // disc capacity. A larger group disproves the generic estimate.
        group_size.max(fallback_episodes_per_disc)
    } else {
        fallback_episodes_per_disc
    };
    let uses_capacity_fallback =
        !has_authored_play_all_group || group_size < fallback_episodes_per_disc;
    let Some(mut range) =
        inferred_episode_range(episode_count, episodes_per_disc, group_size, hint)
    else {
        return ProbableSuggestions {
            disc_identity: Some(format!(
                "Authored label: season {}, volume {}, disc {}; the disc number is inconsistent with the estimated package layout",
                hint.season,
                hint.volume.unwrap_or_default(),
                hint.disc
            )),
            ..Default::default()
        };
    };
    if has_authored_play_all_group {
        range.count = group_size.min(episode_count.saturating_sub(range.first.saturating_sub(1)));
    }
    let positional_first = range.first;
    let alignment = groups
        .first()
        .and_then(|group| runtime_alignment(titles, group, episodes, positional_first));
    if let Some(alignment) = alignment {
        range.first = alignment.first;
        let available = episode_count.saturating_sub(range.first).saturating_add(1);
        range.count = if has_authored_play_all_group {
            group_size.min(available)
        } else {
            range.count.min(available)
        };
    }
    let volume_aware = hint.volume.is_some();
    let stream_layout_matches = groups
        .first()
        .is_some_and(|group| matching_stream_layout(titles, group));
    let base_confidence = match (groups.len(), volume_aware, alignment.is_some()) {
        // Volume labels do not encode the total number of volumes, so these
        // must remain visibly less confident than a disc-only sequence.
        (1, true, true) => 76,
        (1, false, true) => 80,
        (1, true, false) => 64,
        (1, false, false) => 68,
        (_, true, true) => 66,
        (_, false, true) => 70,
        (_, true, false) => 54,
        (_, false, false) => 58,
    };
    let confidence = base_confidence + u8::from(stream_layout_matches) * 6;
    let provider_label = series.providers.join(" + ");
    let method = match (groups.len(), volume_aware, alignment.is_some()) {
        (1, true, true) => {
            "Authored season/volume/disc label + DVD play-all group + global TMDB runtime alignment"
        }
        (1, false, true) => {
            "Season/disc label + DVD play-all group + global TMDB runtime alignment"
        }
        (_, true, true) => {
            "Authored season/volume/disc label + alternate DVD groups + global TMDB runtime alignment"
        }
        (_, false, true) => {
            "Season/disc label + alternate DVD groups + global TMDB runtime alignment"
        }
        (1, true, false) => {
            "Authored season/volume/disc label + hierarchical volume/disc estimate + DVD play-all group + TMDB order"
        }
        (1, false, false) => "Season/disc label + DVD play-all group + TMDB episode order",
        (_, true, false) => {
            "Authored season/volume/disc label + hierarchical volume/disc estimate + alternate DVD play-all groups + TMDB order"
        }
        (_, false, false) => {
            "Season/disc label + alternate DVD play-all groups + TMDB episode order"
        }
    }
    .replace("TMDB", &provider_label);
    let mut evidence = vec![
        match hint.volume {
            Some(volume) => format!(
                "Disc label parsed as season {}, volume {volume}, disc {}",
                hint.season, hint.disc
            ),
            None => format!(
                "Disc label parsed as season {}, disc {}",
                hint.season, hint.disc
            ),
        },
        format!(
            "{provider_label} lists {episode_count} episodes in season {}",
            hint.season,
        ),
        format!(
            "Detected {} probable authored episode group(s); the first contains {group_size} title(s)",
            groups.len()
        ),
    ];
    if has_authored_play_all_group {
        evidence.push(format!(
            "The authored play-all group contains {group_size} complete episode title(s); this direct disc evidence is used instead of a generic capacity estimate"
        ));
        if uses_capacity_fallback {
            evidence.push(format!(
                "Because this may be a partially filled final disc, a 180-minute fallback is used only to estimate the package position and {} total disc(s); it does not cap this disc's {group_size} titles",
                range.estimated_total_discs
            ));
        } else {
            evidence.push(format!(
                "Using the observed group size produces an estimate of {} disc(s) for the season",
                range.estimated_total_discs
            ));
        }
    } else {
        evidence.push(format!(
            "Median candidate runtime is about {} minutes; without an authored play-all group, a 180-minute fallback estimates {} episode(s) per disc and {} disc(s) for the season",
            median_runtime / 60_000,
            episodes_per_disc,
            range.estimated_total_discs
        ));
    }
    if stream_layout_matches {
        evidence.push(format!(
            "All {group_size} candidate titles have the same audio/subtitle codec, language, and channel layout"
        ));
    }
    if let Some(alignment) = alignment {
        evidence.push(format!(
            "Compared {0} DVD title runtimes with every consecutive {0}-episode {provider_label} window; S{1:02}E{2:02} was the unique best fit (mean adjusted error about {3} seconds, timing scale {4:.1}%)",
            alignment.compared_episodes,
            hint.season,
            alignment.first,
            alignment.runtime_error_ms / 1_000,
            alignment.scale_per_mille as f64 / 10.0,
        ));
        if alignment.first != positional_first {
            evidence.push(format!(
                "Runtime evidence shifted the position-only estimate from S{:02}E{:02} by {} episode(s)",
                hint.season,
                positional_first,
                alignment.first.abs_diff(positional_first),
            ));
        }
        if let (Some(first), Some(score)) =
            (alignment.runner_up_first, alignment.runner_up_score_ms)
        {
            evidence.push(format!(
                "The next-best window began at S{:02}E{:02} and scored about {} seconds per title after positional weighting",
                hint.season,
                first,
                score / 1_000,
            ));
        }
    } else if groups.first().is_some_and(|group| group.len() >= 2)
        && episodes
            .iter()
            .filter(|episode| episode.runtime.is_some())
            .count()
            >= 2
    {
        evidence.push(
            format!("Compared the individual {provider_label} episode runtimes, but no window was clearly better than the alternatives; retained the disc-position estimate"),
        );
    }
    let mut assumptions = vec![
        format!("Episode-like titles follow the authored playback order and {provider_label} episode order"),
        "Episodes are distributed as evenly as possible, with earlier groups receiving any remainder"
            .to_owned(),
    ];
    if uses_capacity_fallback {
        assumptions.push("A video DVD holds about 180 minutes of episode content".to_owned());
    }
    if let (Some(volume), Some(discs_in_volume)) = (hint.volume, range.estimated_discs_in_volume) {
        assumptions.insert(
            0,
            format!(
                "Volume {volume} is assumed to be the final volume in this season set; the label does not prove the total number of volumes"
            ),
        );
        assumptions.push(format!(
            "The estimated {} season discs are split across {volume} volume(s), placing {discs_in_volume} disc(s) in volume {volume}",
            range.estimated_total_discs
        ));
    }
    let mut suggestions = Vec::new();
    for group in groups {
        for (offset, title_index) in group.into_iter().enumerate() {
            if offset as u32 >= range.count {
                break;
            }
            let episode_number = range.first.saturating_add(offset as u32);
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
            let name = format!(
                "{} - S{:02}E{episode_number:02} - {}",
                series.title, hint.season, episode.name
            );
            suggestions.push(VideoMetadataSuggestion {
                title_id: title.id,
                name: name.clone(),
                tags: video_tags(
                    &name,
                    &series.title,
                    series.year,
                    Some(hint.season),
                    Some(episode_number),
                    VideoProviderIds::from(series),
                ),
                metadata: VideoMetadata {
                    attribution: MetadataAttribution::new(
                        series.providers.join(" + "),
                        confidence,
                        &method,
                    ),
                    content_type: Some("ProbableEpisode".to_owned()),
                    year: series.year,
                    season: Some(hint.season),
                    episode: Some(episode_number),
                    tmdb_id: series.tmdb_id,
                    tvdb_id: series.tvdb_id,
                    tvmaze_id: series.tvmaze_id,
                },
                evidence: evidence.clone(),
                assumptions: assumptions.clone(),
                episode_choices: episodes
                    .iter()
                    .map(|choice| {
                        let choice_name = format!(
                            "{} - S{:02}E{:02} - {}",
                            series.title, hint.season, choice.episode_number, choice.name
                        );
                        VideoEpisodeChoice {
                            episode: choice.episode_number,
                            tags: video_tags(
                                &choice_name,
                                &series.title,
                                series.year,
                                Some(hint.season),
                                Some(choice.episode_number),
                                VideoProviderIds::from(series),
                            ),
                            name: choice_name,
                        }
                    })
                    .collect(),
            });
        }
    }
    let disc_identity = Some(
        if let (Some(volume), Some(discs_in_volume)) =
            (hint.volume, range.estimated_discs_in_volume)
        {
            format!(
                "Authored label: season {}, volume {volume}, disc {}; proposed S{:02}E{:02}–E{:02}; assumes volume {volume} is the last of {volume}, with {discs_in_volume} disc(s) in this volume (inferred overall disc {} of {})",
                hint.season,
                hint.disc,
                hint.season,
                range.first,
                range.first + range.count.saturating_sub(1),
                range.overall_disc,
                range.estimated_total_discs
            )
        } else {
            format!(
                "Authored label: season {}, disc {}; proposed S{:02}E{:02}–E{:02}; estimated {} disc(s) total",
                hint.season,
                hint.disc,
                hint.season,
                range.first,
                range.first + range.count.saturating_sub(1),
                range.estimated_total_discs
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

fn apply_movie_match(titles: &mut [Title], matched: &VideoMatch) {
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
        VideoProviderIds::from(matched),
    );
    apply_title_name(
        title,
        &matched.title,
        VideoMetadata {
            attribution: MetadataAttribution::new(
                matched.providers.join(" + "),
                if matched.providers.len() > 1 { 90 } else { 80 },
                "Exact title + unique runtime match",
            ),
            content_type: Some("MainMovie".to_owned()),
            year: matched.year,
            season: None,
            episode: None,
            tmdb_id: matched.tmdb_id,
            tvdb_id: matched.tvdb_id,
            tvmaze_id: matched.tvmaze_id,
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
                format!("{media_title} - S{season:02}E{episode:02} - {title}")
            }
            (Some(season), Some(episode), None) => {
                format!("{media_title} - S{season:02}E{episode:02}")
            }
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
    provider_ids: VideoProviderIds,
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
    if let Some(tmdb_id) = provider_ids.tmdb {
        tags.push(MediaTag::new("tmdb_id", tmdb_id.to_string()));
    }
    if let Some(tvdb_id) = provider_ids.tvdb {
        tags.push(MediaTag::new("tvdb_id", tvdb_id.to_string()));
    }
    if let Some(tvmaze_id) = provider_ids.tvmaze {
        tags.push(MediaTag::new("tvmaze_id", tvmaze_id.to_string()));
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
    parse_disc_label(value).title
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
        DiscHint, MediaType, ProviderLookup, VideoEpisode, VideoMatch, VideoMetadataLink,
        assign_output_names, disc_hint, disc_title_report, mapped_display_name,
        merge_episode_metadata, parse_video_metadata_link, probable_episode_suggestions,
        provider_matches_agree, reconcile_provider_lookups, searchable_disc_name,
        title_review_score, tvdb::TvdbMediaType, video_metadata_link_provider,
    };

    #[test]
    fn metadata_links_select_the_provider_and_exact_identity() {
        assert_eq!(
            parse_video_metadata_link("https://www.tvmaze.com/shows/82/game-of-thrones"),
            Ok(VideoMetadataLink::Tvmaze { show_id: 82 })
        );
        assert_eq!(
            parse_video_metadata_link(
                "https://thetvdb.com/series/game-of-thrones/seasons/official/2"
            ),
            Ok(VideoMetadataLink::Tvdb {
                media_type: TvdbMediaType::Tv,
                slug: "game-of-thrones".to_owned(),
                season: Some(2),
            })
        );
        assert_eq!(
            parse_video_metadata_link("https://www.thetvdb.com/movies/arrival"),
            Ok(VideoMetadataLink::Tvdb {
                media_type: TvdbMediaType::Movie,
                slug: "arrival".to_owned(),
                season: None,
            })
        );
        assert_eq!(
            video_metadata_link_provider("https://api.tvmaze.com/shows/1?embed=episodes"),
            Ok("TVMaze")
        );

        let linked = parse_video_metadata_link(
            "https://thetvdb.com/series/game-of-thrones/seasons/official/2",
        )
        .unwrap();
        assert_eq!(
            super::disc_hint_with_link(&super::parse_disc_label("JS1Disc1"), Some(&linked)),
            Some(DiscHint {
                season: 2,
                volume: None,
                disc: 1,
            })
        );

        let tvmaze =
            parse_video_metadata_link("https://www.tvmaze.com/shows/82/game-of-thrones").unwrap();
        assert_eq!(
            super::disc_hint_with_link(&super::parse_disc_label("JS1Disc1"), Some(&tvmaze)),
            Some(DiscHint {
                season: 1,
                volume: None,
                disc: 1,
            })
        );

        let movie = parse_video_metadata_link("https://thetvdb.com/movies/arrival").unwrap();
        assert_eq!(
            super::disc_hint_with_link(&super::parse_disc_label("JS1Disc1"), Some(&movie)),
            None
        );
    }

    #[test]
    fn metadata_links_reject_insecure_or_lookalike_hosts() {
        for value in [
            "http://www.tvmaze.com/shows/82/game-of-thrones",
            "https://tvmaze.com.example/shows/82/game-of-thrones",
            "https://thetvdb.com@example.com/series/game-of-thrones",
            "https://thetvdb.com/search?q=game-of-thrones",
            "https://tvmaze.com/shows/not-a-number/example",
        ] {
            assert!(parse_video_metadata_link(value).is_err(), "{value}");
        }
    }

    #[test]
    fn metadata_providers_share_a_conservative_title_review_score() {
        assert_eq!(
            title_review_score("Supernatural", "Supernatural"),
            Some(100)
        );
        assert!(
            title_review_score("Supernaturall", "Supernatural").is_some_and(|score| score > 90)
        );
        assert_eq!(title_review_score("Supernatural", "Superstore"), None);
    }

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
                tvdb_id: None,
                tvmaze_id: None,
            }),
            output_file: String::new(),
            selected: false,
        }
    }

    #[test]
    fn enriched_names_become_safe_unique_output_names() {
        let mut titles = vec![
            title("Example Show - S01E01 - Pilot", true),
            title("Example Show - S01E01 - Pilot", true),
        ];
        titles[1].source_title_id = "00801".into();

        assign_output_names(&mut titles, "DISC");

        assert_eq!(titles[0].output_file, "Example Show - S01E01 - Pilot.mkv");
        assert_eq!(
            titles[1].output_file,
            "Example Show - S01E01 - Pilot_00801.mkv"
        );
    }

    #[test]
    fn episode_report_keeps_stream_data_for_unmatched_disc_titles() {
        use media_rip_core::{StreamKind, Track};

        let mut unmatched = title("Unmatched title", false);
        unmatched.tracks.push(Track {
            index: 2,
            kind: StreamKind::Audio,
            codec: "ac3".to_owned(),
            codec_long_name: Some("ATSC A/52A".to_owned()),
            language: Some("eng".to_owned()),
            name: None,
            properties: vec!["stereo".to_owned()],
            selected: true,
        });

        let report = disc_title_report(&unmatched, &[]);

        assert_eq!(report["match_status"], "unmatched");
        assert_eq!(report["disc_title"]["tracks"][0]["codec"], "ac3");
        assert_eq!(report["disc_title"]["tracks"][0]["language"], "eng");
    }

    #[test]
    fn formats_mapped_episode_names_with_show_and_ascii_dashes() {
        let item = super::discdb::DiscDbItem {
            title: "Pilot".into(),
            item_type: "Episode".into(),
            season: Some("1".into()),
            episode: Some("2".into()),
        };

        assert_eq!(
            mapped_display_name(&item, "Example Show", Some(1), Some(2)),
            "Example Show - S01E02 - Pilot"
        );
    }

    #[test]
    fn parses_extensible_disc_label_patterns_before_searching() {
        let cases = [
            ("BAND_OF_BROTHERS_S1D1", "BAND OF BROTHERS", 1, None, 1),
            ("SUPERNATURAL_S4_D5", "SUPERNATURAL", 4, None, 5),
            ("SUPERNATURAL_SEASON3_DISC1", "SUPERNATURAL", 3, None, 1),
            ("Supernatural Season 3 Disk 1", "Supernatural", 3, None, 1),
            ("Example.S04.V02.D03.BD", "Example", 4, Some(2), 3),
            ("Example-S4V2D3-Blu-ray", "Example", 4, Some(2), 3),
            (
                "Example season4 volume2 disk3 DVD",
                "Example",
                4,
                Some(2),
                3,
            ),
        ];
        for (label, title, season, volume, disc) in cases {
            assert_eq!(searchable_disc_name(label), title, "label: {label}");
            assert_eq!(
                disc_hint(label),
                Some(DiscHint {
                    season,
                    volume,
                    disc,
                }),
                "label: {label}"
            );
        }

        assert_eq!(searchable_disc_name("Inception-DISC1"), "Inception");
        assert_eq!(disc_hint("Inception-DISC1"), None);
        let opaque = super::parse_disc_label("JS1Disc1");
        assert_eq!(opaque.title, "JS1");
        assert_eq!(opaque.season, None);
        assert_eq!(opaque.disc, Some(1));
        assert_eq!(
            searchable_disc_name("Season of the Witch"),
            "Season of the Witch"
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
        let series = VideoMatch {
            providers: vec!["TMDB"],
            media_type: MediaType::Tv,
            title: "Supernatural".into(),
            year: Some(2005),
            runtime_minutes: Some(42),
            tmdb_id: Some(1_622),
            tvdb_id: None,
            tvmaze_id: None,
        };
        let episodes = (1..=22)
            .map(|episode_number| VideoEpisode {
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
        assert_eq!(
            probable.suggestions[0].name,
            "Supernatural - S02E20 - Episode 20"
        );
        assert_eq!(
            probable.suggestions[3].name,
            "Supernatural - S02E20 - Episode 20"
        );
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

    #[test]
    fn volume_two_disc_one_starts_at_episode_twelve() {
        let episode_runtime = 42 * 60_000;
        let mut titles = (0..5)
            .map(|id| {
                let duration = if id == 0 {
                    episode_runtime * 4
                } else {
                    episode_runtime
                };
                let mut title = title(&format!("Title {}", id + 1), false);
                title.id = id;
                title.duration_ms = duration;
                if id > 0 {
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
        let series = VideoMatch {
            providers: vec!["TMDB"],
            media_type: MediaType::Tv,
            title: "Supernatural".into(),
            year: Some(2005),
            runtime_minutes: Some(42),
            tmdb_id: Some(1_622),
            tvdb_id: None,
            tvmaze_id: None,
        };
        let episodes = (1..=22)
            .map(|episode_number| VideoEpisode {
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
                disc: 1,
            },
        );

        assert_eq!(probable.suggestions.len(), 4);
        assert_eq!(
            probable
                .suggestions
                .iter()
                .map(|suggestion| suggestion.metadata.episode)
                .collect::<Vec<_>>(),
            [Some(12), Some(13), Some(14), Some(15)]
        );
        assert!(
            probable.suggestions[0]
                .assumptions
                .iter()
                .any(|assumption| assumption.contains("final volume"))
        );
        assert!(
            probable
                .disc_identity
                .as_deref()
                .is_some_and(|identity| identity.contains("S02E12–E15"))
        );

        // Generating a proposal must not mutate the scanned title metadata.
        assert!(
            titles
                .iter_mut()
                .all(|title| title.video_metadata.is_none())
        );
    }

    #[test]
    fn authored_five_episode_group_overrides_generic_dvd_capacity() {
        use media_rip_core::{StreamKind, Track};

        let episode_durations = [2_430_840, 2_435_240, 2_433_640, 2_435_840, 2_439_440];
        let mut titles = Vec::new();
        let mut play_all = title("Play all", false);
        play_all.duration_ms = episode_durations.iter().sum();
        titles.push(play_all);
        for (index, duration) in episode_durations.into_iter().enumerate() {
            let mut episode = title(&format!("Title {}", index + 2), false);
            episode.id = index as u32 + 1;
            episode.duration_ms = duration;
            episode.chapters = vec![
                Chapter {
                    index: 1,
                    start_ms: 0,
                    end_ms: duration / 2,
                    name: None,
                },
                Chapter {
                    index: 2,
                    start_ms: duration / 2,
                    end_ms: duration,
                    name: None,
                },
            ];
            episode.tracks = vec![
                Track {
                    index: 1,
                    kind: StreamKind::Audio,
                    codec: "AC3".to_owned(),
                    codec_long_name: None,
                    language: Some("eng".to_owned()),
                    name: None,
                    properties: vec!["5.1(side)".to_owned(), "40:31".to_owned()],
                    selected: true,
                },
                Track {
                    index: 2,
                    kind: StreamKind::Subtitle,
                    codec: "DVD_SUBTITLE".to_owned(),
                    codec_long_name: None,
                    language: Some("eng".to_owned()),
                    name: None,
                    properties: Vec::new(),
                    selected: true,
                },
            ];
            titles.push(episode);
        }
        let series = VideoMatch {
            providers: vec!["TMDB"],
            media_type: MediaType::Tv,
            title: "Gossip Girl".into(),
            year: Some(2007),
            runtime_minutes: Some(42),
            tmdb_id: Some(13_983),
            tvdb_id: None,
            tvmaze_id: None,
        };
        let episodes = (1..=22)
            .map(|episode_number| VideoEpisode {
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
                season: 4,
                volume: None,
                disc: 1,
            },
        );

        assert_eq!(probable.suggestions.len(), 5);
        assert_eq!(
            probable
                .suggestions
                .iter()
                .map(|suggestion| suggestion.metadata.episode)
                .collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3), Some(4), Some(5)]
        );
        let first = &probable.suggestions[0];
        assert_eq!(first.metadata.attribution.confidence_percent, 74);
        assert!(
            first
                .evidence
                .iter()
                .any(|evidence| evidence.contains("same audio/subtitle"))
        );
        assert!(
            first
                .assumptions
                .iter()
                .all(|assumption| !assumption.contains("180 minutes"))
        );
    }

    #[test]
    fn episode_runtimes_can_correct_a_one_episode_partition_offset() {
        let dvd_minutes = [40_u64, 44, 41, 46];
        let titles = (0..=dvd_minutes.len())
            .map(|id| {
                let duration = if id == 0 {
                    dvd_minutes.iter().sum::<u64>() * 60_000
                } else {
                    dvd_minutes[id - 1] * 60_000
                };
                let mut title = title(&format!("Title {}", id + 1), false);
                title.id = id as u32;
                title.duration_ms = duration;
                if id > 0 {
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
        let series = VideoMatch {
            providers: vec!["TMDB"],
            media_type: MediaType::Tv,
            title: "Supernatural".into(),
            year: Some(2005),
            runtime_minutes: Some(42),
            tmdb_id: Some(1_622),
            tvdb_id: None,
            tvmaze_id: None,
        };
        let runtimes = [
            42, 42, 42, 42, 42, 42, 42, 42, 42, 42, 40, 44, 41, 46, 43, 39, 45, 42, 40, 43, 41, 44,
        ];
        let episodes = runtimes
            .into_iter()
            .enumerate()
            .map(|(index, runtime)| VideoEpisode {
                episode_number: index as u32 + 1,
                name: format!("Episode {}", index + 1),
                runtime: Some(runtime),
            })
            .collect::<Vec<_>>();

        let probable = probable_episode_suggestions(
            &titles,
            &series,
            &episodes,
            DiscHint {
                season: 2,
                volume: Some(2),
                disc: 1,
            },
        );

        assert_eq!(
            probable
                .suggestions
                .iter()
                .map(|suggestion| suggestion.metadata.episode)
                .collect::<Vec<_>>(),
            [Some(11), Some(12), Some(13), Some(14)]
        );
        assert!(
            probable.suggestions[0]
                .evidence
                .iter()
                .any(|evidence| evidence.contains("shifted the position-only estimate"))
        );
        assert_eq!(
            probable.suggestions[0]
                .metadata
                .attribution
                .confidence_percent,
            76
        );
    }

    #[test]
    fn provider_agreement_combines_ids_and_raises_confidence() {
        let tmdb = ProviderLookup {
            match_method: "exact title",
            matched: VideoMatch {
                providers: vec!["TMDB"],
                media_type: MediaType::Tv,
                title: "The Example".into(),
                year: Some(2005),
                runtime_minutes: Some(42),
                tmdb_id: Some(10),
                tvdb_id: None,
                tvmaze_id: None,
            },
            episodes: vec![VideoEpisode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: Some(42),
            }],
            artwork: None,
            warnings: Vec::new(),
        };
        let tvdb = ProviderLookup {
            match_method: "exact title + DVD order",
            matched: VideoMatch {
                providers: vec!["TVDB"],
                media_type: MediaType::Tv,
                title: "Example".into(),
                year: Some(2005),
                runtime_minutes: None,
                tmdb_id: None,
                tvdb_id: Some(20),
                tvmaze_id: None,
            },
            episodes: vec![VideoEpisode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: None,
            }],
            artwork: None,
            warnings: Vec::new(),
        };
        let tvmaze = ProviderLookup {
            match_method: "exact title",
            matched: VideoMatch {
                providers: vec!["TVMaze"],
                media_type: MediaType::Tv,
                title: "Example".into(),
                year: Some(2005),
                runtime_minutes: Some(42),
                tmdb_id: None,
                tvdb_id: None,
                tvmaze_id: Some(30),
            },
            episodes: vec![VideoEpisode {
                episode_number: 1,
                name: "Pilot".into(),
                runtime: Some(42),
            }],
            artwork: None,
            warnings: Vec::new(),
        };

        assert!(provider_matches_agree(&tmdb.matched, &tvdb.matched));
        let (resolved, _, attributions, warnings) =
            reconcile_provider_lookups(Ok(Some(tmdb)), Ok(Some(tvdb)), Ok(Some(tvmaze)), false);
        let resolved = resolved.unwrap();

        assert!(warnings.is_empty());
        assert_eq!(resolved.matched.tmdb_id, Some(10));
        assert_eq!(resolved.matched.tvdb_id, Some(20));
        assert_eq!(resolved.matched.tvmaze_id, Some(30));
        assert_eq!(resolved.episodes[0].runtime, Some(42));
        assert_eq!(attributions.len(), 3);
        assert!(
            attributions
                .iter()
                .all(|attribution| attribution.confidence_percent == 88)
        );
    }

    #[test]
    fn episode_merge_does_not_mix_runtimes_across_different_orders() {
        let merged = merge_episode_metadata(
            vec![VideoEpisode {
                episode_number: 1,
                name: "Broadcast Pilot".into(),
                runtime: Some(42),
            }],
            vec![VideoEpisode {
                episode_number: 1,
                name: "DVD Feature".into(),
                runtime: None,
            }],
        );

        assert_eq!(merged[0].name, "DVD Feature");
        assert_eq!(merged[0].runtime, None);
    }
}
