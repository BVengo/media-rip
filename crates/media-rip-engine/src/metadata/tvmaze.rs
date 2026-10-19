use std::{io::Read, time::Duration};

use media_rip_core::{CoverArt, ports::CancellationToken};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    cache::MetadataCache, clear_title_review_winner, title_review_score, tmdb::normalize_title,
};
use crate::diagnostics;

const API_ROOT: &str = "https://api.tvmaze.com";
const CACHE_NAMESPACE: &str = "tvmaze-raw-v1";
const MAX_ARTWORK_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(super) struct TvmazeMatch {
    pub id: u64,
    pub title: String,
    pub year: Option<u32>,
    pub runtime_minutes: Option<u64>,
    pub artwork_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TvmazeEpisode {
    pub episode_number: u32,
    pub name: String,
    pub runtime: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResult {
    show: Show,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Show {
    id: u64,
    name: String,
    premiered: Option<String>,
    runtime: Option<u64>,
    average_runtime: Option<u64>,
    image: Option<Image>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Image {
    medium: Option<String>,
    original: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Episode {
    name: String,
    season: Option<u32>,
    number: Option<u32>,
    runtime: Option<u64>,
}

pub(super) struct TvmazeClient<'a> {
    cache: &'a MetadataCache,
    cancellation: &'a CancellationToken,
}

impl<'a> TvmazeClient<'a> {
    pub(super) fn new(cache: &'a MetadataCache, cancellation: &'a CancellationToken) -> Self {
        Self {
            cache,
            cancellation,
        }
    }

    pub(super) fn search_choices(&self, query: &str) -> Result<Vec<TvmazeMatch>, String> {
        let results: Vec<SearchResult> = self.get_json("/search/shows", &[("q", query)])?;
        Ok(results
            .into_iter()
            .map(|result| result.show.into_match())
            .collect())
    }

    pub(super) fn search_exact(&self, query: &str) -> Result<Option<TvmazeMatch>, String> {
        let results: Vec<SearchResult> = self.get_json("/search/shows", &[("q", query)])?;
        let query_year = year_in(query);
        let query_name = normalize_title(&without_year(query));
        let mut exact = results
            .into_iter()
            .map(|result| result.show)
            .filter(|show| normalize_title(&show.name) == query_name)
            .filter(|show| query_year.is_none_or(|year| show.year() == Some(year)))
            .collect::<Vec<_>>();
        if exact.len() != 1 {
            return Ok(None);
        }
        Ok(Some(exact.remove(0).into_match()))
    }

    pub(super) fn show(&self, id: u64) -> Result<TvmazeMatch, String> {
        self.get_json::<Show>(&format!("/shows/{id}"), &[])
            .map(Show::into_match)
    }

    pub(super) fn lookup_imdb(&self, imdb_id: &str) -> Result<Option<TvmazeMatch>, String> {
        self.get_optional_json::<Show>("/lookup/shows", &[("imdb", imdb_id)])
            .map(|show| show.map(Show::into_match))
    }

    pub(super) fn search_tv_season_review(
        &self,
        query: &str,
        season: u32,
    ) -> Result<Option<(TvmazeMatch, Vec<TvmazeEpisode>, bool)>, String> {
        let results: Vec<SearchResult> = self.get_json("/search/shows", &[("q", query)])?;
        let query_year = year_in(query);
        let mut matches = Vec::new();
        for show in results.into_iter().map(|result| result.show) {
            if query_year.is_some_and(|year| show.year() != Some(year)) {
                continue;
            }
            let Some(score) = title_review_score(&without_year(query), &show.name) else {
                continue;
            };
            let episodes = self.season(show.id, season)?;
            if !episodes.is_empty() {
                matches.push((score, show.into_match(), episodes));
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
        let (_, matched, episodes) = matches.remove(0);
        Ok(Some((matched, episodes, scores[0] == 100)))
    }

    pub(super) fn season(&self, id: u64, season: u32) -> Result<Vec<TvmazeEpisode>, String> {
        let episodes: Vec<Episode> = self.get_json(&format!("/shows/{id}/episodes"), &[])?;
        Ok(episodes
            .into_iter()
            .filter(|episode| episode.season == Some(season))
            .filter_map(|episode| {
                Some(TvmazeEpisode {
                    episode_number: episode.number?,
                    name: episode.name,
                    runtime: episode.runtime,
                })
            })
            .collect())
    }

    pub(super) fn artwork(&self, artwork_url: &str) -> Result<CoverArt, String> {
        if !artwork_url.starts_with("https://") {
            return Err("TVMaze returned a non-HTTPS artwork URL".to_owned());
        }
        if let Some(data) = self.cache.read_bytes("tvmaze-artwork", artwork_url) {
            return supported_artwork(data);
        }
        if self.cancellation.is_cancelled() {
            return Err("TVMaze artwork lookup cancelled".to_owned());
        }
        let response = ureq::get(artwork_url)
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
            return Err("TVMaze artwork was empty or exceeded 10 MiB".to_owned());
        }
        let artwork = supported_artwork(data)?;
        self.cache
            .write_bytes("tvmaze-artwork", artwork_url, &artwork.data);
        Ok(artwork)
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, String> {
        let cache_key = cache_key(path, query);
        if let Some(cached) = self
            .cache
            .read::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
        {
            return diagnostics::from_value("TVMaze", &cache_key, "cache", cached);
        }
        match self.request(path, query) {
            Ok(value) => {
                self.cache.write(CACHE_NAMESPACE, &cache_key, &value);
                diagnostics::from_value("TVMaze", &cache_key, "network", value)
            }
            Err(error) => {
                let stale = self
                    .cache
                    .read_stale::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
                    .ok_or(error)?;
                diagnostics::from_value("TVMaze", &cache_key, "stale-cache", stale)
            }
        }
    }

    fn get_optional_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Option<T>, String> {
        let cache_key = cache_key(path, query);
        if let Some(cached) = self
            .cache
            .read::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
        {
            return diagnostics::from_value("TVMaze", &cache_key, "cache", cached).map(Some);
        }
        if self.cancellation.is_cancelled() {
            return Err("TVMaze lookup cancelled".to_owned());
        }
        let mut request = self.request_builder(path);
        for (key, value) in query {
            request = request.query(key, value);
        }
        match request.call() {
            Ok(response) => {
                let value = response
                    .into_json::<serde_json::Value>()
                    .map_err(|error| error.to_string())?;
                self.cache.write(CACHE_NAMESPACE, &cache_key, &value);
                diagnostics::from_value("TVMaze", &cache_key, "network", value).map(Some)
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(error) => {
                let stale = self
                    .cache
                    .read_stale::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
                    .ok_or_else(|| error.to_string())?;
                diagnostics::from_value("TVMaze", &cache_key, "stale-cache", stale).map(Some)
            }
        }
    }

    fn request(&self, path: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, String> {
        if self.cancellation.is_cancelled() {
            return Err("TVMaze lookup cancelled".to_owned());
        }
        let mut request = self.request_builder(path);
        for (key, value) in query {
            request = request.query(key, value);
        }
        request
            .call()
            .map_err(|error| error.to_string())?
            .into_json::<serde_json::Value>()
            .map_err(|error| error.to_string())
    }

    fn request_builder(&self, path: &str) -> ureq::Request {
        ureq::get(&format!("{API_ROOT}{path}"))
            .set(
                "User-Agent",
                concat!("MediaRip/", env!("CARGO_PKG_VERSION")),
            )
            .set("Accept", "application/json")
            .timeout(Duration::from_secs(10))
    }
}

impl Show {
    fn year(&self) -> Option<u32> {
        self.premiered
            .as_deref()
            .and_then(|date| date.get(..4))
            .and_then(|year| year.parse().ok())
    }

    fn into_match(self) -> TvmazeMatch {
        let year = self.year();
        TvmazeMatch {
            id: self.id,
            title: self.name,
            year,
            runtime_minutes: self.runtime.or(self.average_runtime),
            artwork_url: self.image.and_then(|image| image.original.or(image.medium)),
        }
    }
}

fn cache_key(path: &str, query: &[(&str, &str)]) -> String {
    format!(
        "{path}?{}",
        serde_json::to_string(query).unwrap_or_default()
    )
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

fn supported_artwork(data: Vec<u8>) -> Result<CoverArt, String> {
    let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        return Err("TVMaze returned unsupported artwork".to_owned());
    };
    Ok(CoverArt::new(mime, data))
}

#[cfg(test)]
mod tests {
    use super::Show;

    #[test]
    fn parses_show_runtime_year_and_preferred_artwork() {
        let show: Show = serde_json::from_str(
            r#"{"id":82,"name":"Game of Thrones","premiered":"2011-04-17","runtime":60,"averageRuntime":57,"image":{"medium":"https://example/medium.jpg","original":"https://example/original.jpg"}}"#,
        )
        .unwrap();
        let matched = show.into_match();

        assert_eq!(matched.id, 82);
        assert_eq!(matched.year, Some(2011));
        assert_eq!(matched.runtime_minutes, Some(60));
        assert_eq!(
            matched.artwork_url.as_deref(),
            Some("https://example/original.jpg")
        );
    }
}
