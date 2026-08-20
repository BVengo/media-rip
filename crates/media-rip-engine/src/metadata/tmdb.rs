use std::{io::Read, time::Duration};

use media_rip_core::{CoverArt, ports::CancellationToken};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{cache::MetadataCache, clear_title_review_winner, title_review_score};
use crate::diagnostics;

const API_ROOT: &str = "https://api.themoviedb.org/3";
const CACHE_NAMESPACE: &str = "tmdb-raw-v1";
const IMAGE_ROOT: &str = "https://image.tmdb.org/t/p/w500";
const MAX_ARTWORK_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MediaType {
    Movie,
    Tv,
}

impl MediaType {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "tv",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TmdbMatch {
    pub id: u64,
    pub media_type: MediaType,
    pub title: String,
    pub year: Option<u32>,
    pub runtime_minutes: Option<u64>,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TmdbEpisode {
    pub episode_number: u32,
    pub name: String,
    pub runtime: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SeasonResponse {
    #[serde(default)]
    episodes: Vec<TmdbEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResult {
    id: u64,
    media_type: String,
    title: Option<String>,
    name: Option<String>,
    original_title: Option<String>,
    original_name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DetailsResponse {
    id: u64,
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    runtime: Option<u64>,
    #[serde(default)]
    episode_run_time: Vec<u64>,
    poster_path: Option<String>,
}

pub(super) struct TmdbClient<'a> {
    token: &'a str,
    cache: &'a MetadataCache,
    cancellation: &'a CancellationToken,
}

impl<'a> TmdbClient<'a> {
    pub(super) fn new(
        token: &'a str,
        cache: &'a MetadataCache,
        cancellation: &'a CancellationToken,
    ) -> Self {
        Self {
            token,
            cache,
            cancellation,
        }
    }

    pub(super) fn details(&self, id: u64, media_type: MediaType) -> Result<TmdbMatch, String> {
        let path = format!("/{}/{id}", media_type.as_str());
        let details: DetailsResponse = self.get_json(&path, &[])?;
        Ok(match_from_details(details, media_type))
    }

    pub(super) fn search_exact(&self, query: &str) -> Result<Option<TmdbMatch>, String> {
        let mut exact = self.exact_search_results(query)?;
        if exact.len() != 1 {
            return Ok(None);
        }
        let result = exact.remove(0);
        let media_type = if result.media_type == "movie" {
            MediaType::Movie
        } else {
            MediaType::Tv
        };
        self.details(result.id, media_type).map(Some)
    }

    pub(super) fn search_tv_season_review(
        &self,
        query: &str,
        season: u32,
    ) -> Result<Option<(TmdbMatch, Vec<TmdbEpisode>, bool)>, String> {
        let query_year = year_in(query);
        let candidates = self
            .search_results(query)?
            .into_iter()
            .filter(|result| result.media_type == "tv")
            .filter(|result| query_year.is_none_or(|year| result.year() == Some(year)))
            .filter_map(|result| {
                let score = result.title_review_score(&without_year(query))?;
                Some((score, result))
            })
            .collect::<Vec<_>>();
        let mut matches = Vec::new();
        for (score, candidate) in candidates {
            if let Ok(episodes) = self.season(candidate.id, season)
                && !episodes.is_empty()
            {
                matches.push((score, candidate, episodes));
            }
        }
        matches.sort_by_key(|(score, _, _)| std::cmp::Reverse(*score));
        let scores = matches
            .iter()
            .map(|(score, _, _)| *score)
            .collect::<Vec<_>>();
        if !clear_title_review_winner(&scores) {
            return Ok(None);
        }
        let (_, result, episodes) = matches.remove(0);
        Ok(Some((
            self.details(result.id, MediaType::Tv)?,
            episodes,
            scores[0] == 100,
        )))
    }

    fn exact_search_results(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        let results = self.search_results(query)?;
        let query_year = year_in(query);
        let query_name = normalize_title(&without_year(query));
        Ok(results
            .into_iter()
            .filter(|result| result.matches_normalized_title(&query_name))
            .filter(|result| query_year.is_none_or(|year| result.year() == Some(year)))
            .collect())
    }

    fn search_results(&self, query: &str) -> Result<Vec<SearchResult>, String> {
        let response: SearchResponse = self.get_json(
            "/search/multi",
            &[
                ("query", query),
                ("include_adult", "false"),
                ("language", "en-US"),
            ],
        )?;
        Ok(response
            .results
            .into_iter()
            .filter(|result| matches!(result.media_type.as_str(), "movie" | "tv"))
            .collect())
    }

    pub(super) fn season(&self, id: u64, season: u32) -> Result<Vec<TmdbEpisode>, String> {
        let response: SeasonResponse = self.get_json(
            &format!("/tv/{id}/season/{season}"),
            &[("language", "en-US")],
        )?;
        Ok(response.episodes)
    }

    pub(super) fn artwork(&self, poster_path: &str) -> Result<CoverArt, String> {
        let key = poster_path.trim_start_matches('/');
        if let Some(data) = self.cache.read_bytes("tmdb-artwork", key) {
            return Ok(CoverArt::new(mime_type(&data), data));
        }
        if self.cancellation.is_cancelled() {
            return Err("TMDB artwork lookup cancelled".to_owned());
        }
        let response = ureq::get(&format!("{IMAGE_ROOT}/{}", key))
            .set(
                "User-Agent",
                concat!("MediaRip/", env!("CARGO_PKG_VERSION")),
            )
            .set("Accept", "image/jpeg,image/png")
            .timeout(Duration::from_secs(10))
            .call()
            .map_err(|error| error.to_string())?;
        let mut data = Vec::new();
        response
            .into_reader()
            .take(MAX_ARTWORK_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|error| error.to_string())?;
        if data.is_empty() || data.len() as u64 > MAX_ARTWORK_BYTES {
            return Err("TMDB artwork was empty or exceeded 10 MiB".to_owned());
        }
        let mime = mime_type(&data);
        if mime == "application/octet-stream" {
            return Err("TMDB returned unsupported artwork".to_owned());
        }
        self.cache.write_bytes("tmdb-artwork", key, &data);
        Ok(CoverArt::new(mime, data))
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, String> {
        let cache_key = format!(
            "{path}?{}",
            serde_json::to_string(query).unwrap_or_default()
        );
        if let Some(cached) = self
            .cache
            .read::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
        {
            return diagnostics::from_value("TMDB", &cache_key, "cache", cached);
        }
        if self.cancellation.is_cancelled() {
            return Err("TMDB lookup cancelled".to_owned());
        }
        let mut request = ureq::get(&format!("{API_ROOT}{path}"))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set(
                "User-Agent",
                concat!("MediaRip/", env!("CARGO_PKG_VERSION")),
            )
            .set("Accept", "application/json")
            .timeout(Duration::from_secs(10));
        for (key, value) in query {
            request = request.query(key, value);
        }
        let network_response =
            request
                .call()
                .map_err(|error| error.to_string())
                .and_then(|response| {
                    response
                        .into_json::<serde_json::Value>()
                        .map_err(|error| error.to_string())
                });
        match network_response {
            Ok(value) => {
                self.cache.write(CACHE_NAMESPACE, &cache_key, &value);
                diagnostics::from_value("TMDB", &cache_key, "network", value)
            }
            Err(error) => {
                let stale = self
                    .cache
                    .read_stale::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
                    .ok_or_else(|| error.to_string())?;
                diagnostics::from_value("TMDB", &cache_key, "stale-cache", stale)
            }
        }
    }
}

impl SearchResult {
    fn titles(&self) -> impl Iterator<Item = &str> {
        [
            self.title.as_deref(),
            self.name.as_deref(),
            self.original_title.as_deref(),
            self.original_name.as_deref(),
        ]
        .into_iter()
        .flatten()
    }

    fn matches_normalized_title(&self, query: &str) -> bool {
        self.titles().any(|title| normalize_title(title) == query)
    }

    fn title_review_score(&self, query: &str) -> Option<u8> {
        self.titles()
            .filter_map(|title| title_review_score(query, title))
            .max()
    }

    fn year(&self) -> Option<u32> {
        self.release_date
            .as_deref()
            .or(self.first_air_date.as_deref())
            .and_then(|date| date.get(..4))
            .and_then(|year| year.parse().ok())
    }
}

fn match_from_details(details: DetailsResponse, media_type: MediaType) -> TmdbMatch {
    let title = details
        .title
        .or(details.name)
        .unwrap_or_else(|| "Unknown title".to_owned());
    let year = details
        .release_date
        .as_deref()
        .or(details.first_air_date.as_deref())
        .and_then(|date| date.get(..4))
        .and_then(|year| year.parse().ok());
    let runtime_minutes = details
        .runtime
        .or_else(|| details.episode_run_time.first().copied());
    TmdbMatch {
        id: details.id,
        media_type,
        title,
        year,
        runtime_minutes,
        poster_path: details.poster_path,
    }
}

pub(super) fn normalize_title(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn year_in(value: &str) -> Option<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .find_map(|part| {
            (part.len() == 4)
                .then(|| part.parse::<u32>().ok())
                .flatten()
                .filter(|year| (1888..=2100).contains(year))
        })
}

fn without_year(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|part| {
            !(part.len() == 4
                && part
                    .parse::<u32>()
                    .is_ok_and(|year| (1888..=2100).contains(&year)))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn mime_type(data: &[u8]) -> &'static str {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_title, year_in};

    #[test]
    fn normalizes_search_titles_and_extracts_a_real_year() {
        assert_eq!(normalize_title("Wall-E: The Movie"), "wallethemovie");
        assert_eq!(year_in("WALL_E_2008_DISC_1"), Some(2008));
        assert_eq!(year_in("DISC_0001"), None);
    }
}
