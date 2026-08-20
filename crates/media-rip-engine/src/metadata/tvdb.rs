use std::{io::Read, time::Duration};

use media_rip_core::{CoverArt, ports::CancellationToken};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{cache::MetadataCache, clear_title_review_winner, title_review_score};
use crate::diagnostics;

const API_ROOT: &str = "https://api4.thetvdb.com/v4";
const CACHE_NAMESPACE: &str = "tvdb-raw-v1";
const IMAGE_ROOT: &str = "https://artworks.thetvdb.com";
const MAX_ARTWORK_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TvdbMediaType {
    Movie,
    Tv,
}

impl TvdbMediaType {
    fn search_type(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Tv => "series",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TvdbMatch {
    pub id: u64,
    pub media_type: TvdbMediaType,
    pub title: String,
    pub year: Option<u32>,
    pub runtime_minutes: Option<u64>,
    pub artwork_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TvdbEpisode {
    pub episode_number: u32,
    pub name: String,
    pub runtime: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoginData {
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResult {
    #[serde(default)]
    aliases: Vec<String>,
    name: Option<String>,
    title: Option<String>,
    name_translated: Option<String>,
    tvdb_id: String,
    #[serde(rename = "type")]
    media_type: String,
    year: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BaseRecord {
    id: u64,
    name: String,
    year: Option<String>,
    runtime: Option<u64>,
    average_runtime: Option<u64>,
    image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EpisodesData {
    series: BaseRecord,
    #[serde(default)]
    episodes: Vec<EpisodeRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EpisodeRecord {
    name: String,
    number: u32,
    season_number: u32,
    runtime: Option<u64>,
}

pub(super) struct TvdbClient<'a> {
    api_key: &'a str,
    subscriber_pin: Option<&'a str>,
    cache: &'a MetadataCache,
    cancellation: &'a CancellationToken,
}

impl<'a> TvdbClient<'a> {
    pub(super) fn new(
        api_key: &'a str,
        subscriber_pin: Option<&'a str>,
        cache: &'a MetadataCache,
        cancellation: &'a CancellationToken,
    ) -> Self {
        Self {
            api_key,
            subscriber_pin,
            cache,
            cancellation,
        }
    }

    pub(super) fn search_exact(&self, query: &str) -> Result<Option<TvdbMatch>, String> {
        let mut exact = self.exact_search_results(query, None, None)?;
        if exact.len() != 1 {
            return Ok(None);
        }
        self.details_from_search(exact.remove(0)).map(Some)
    }

    pub(super) fn lookup_slug(
        &self,
        slug: &str,
        media_type: TvdbMediaType,
    ) -> Result<TvdbMatch, String> {
        let path = match media_type {
            TvdbMediaType::Movie => format!("/movies/slug/{slug}"),
            TvdbMediaType::Tv => format!("/series/slug/{slug}"),
        };
        let details: Envelope<BaseRecord> = self.get_json(&path, &[])?;
        Ok(match_from_base_record(details.data, media_type))
    }

    pub(super) fn search_exact_type(
        &self,
        query: &str,
        media_type: TvdbMediaType,
    ) -> Result<Option<TvdbMatch>, String> {
        let mut exact = self.exact_search_results(query, None, Some(media_type))?;
        if exact.len() != 1 {
            return Ok(None);
        }
        self.details_from_search(exact.remove(0)).map(Some)
    }

    pub(super) fn search_remote(
        &self,
        remote_id: &str,
        media_type: TvdbMediaType,
    ) -> Result<Option<TvdbMatch>, String> {
        let mut results = self.exact_search_results("", Some(remote_id), Some(media_type))?;
        if results.len() != 1 {
            return Ok(None);
        }
        self.details_from_search(results.remove(0)).map(Some)
    }

    pub(super) fn search_tv_season_review(
        &self,
        query: &str,
        season: u32,
    ) -> Result<Option<(TvdbMatch, Vec<TvdbEpisode>, bool)>, String> {
        let query_year = year_in(query);
        let response = self.search_results(query, None, Some(TvdbMediaType::Tv))?;
        let candidates = response
            .into_iter()
            .filter(|result| query_year.is_none_or(|year| result.year() == Some(year)))
            .filter_map(|result| {
                let score = result.title_review_score(&without_year(query))?;
                Some((score, result))
            })
            .collect::<Vec<_>>();
        let mut matches = Vec::new();
        for (score, candidate) in candidates {
            let id = parse_id(&candidate.tvdb_id)?;
            if let Ok(episodes) = self.season(id, season)
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
            self.details_from_search(result)?,
            episodes,
            scores[0] == 100,
        )))
    }

    pub(super) fn season(&self, id: u64, season: u32) -> Result<Vec<TvdbEpisode>, String> {
        // Physical media benefits from TVDB's DVD ordering. Some series only
        // have a default order, so fall back without failing the provider.
        for order in ["dvd", "default"] {
            let response = self.get_json::<Envelope<EpisodesData>>(
                &format!("/series/{id}/episodes/{order}"),
                &[("page", "0"), ("season", &season.to_string())],
            );
            let Ok(response) = response else {
                continue;
            };
            let episodes = response
                .data
                .episodes
                .into_iter()
                .filter(|episode| episode.season_number == season)
                .map(|episode| TvdbEpisode {
                    episode_number: episode.number,
                    name: episode.name,
                    runtime: episode.runtime,
                })
                .collect::<Vec<_>>();
            if !episodes.is_empty() {
                return Ok(episodes);
            }
        }
        Ok(Vec::new())
    }

    pub(super) fn artwork(&self, artwork_url: &str) -> Result<CoverArt, String> {
        let url = if artwork_url.starts_with("http://") || artwork_url.starts_with("https://") {
            artwork_url.to_owned()
        } else {
            format!("{IMAGE_ROOT}/{}", artwork_url.trim_start_matches('/'))
        };
        if let Some(data) = self.cache.read_bytes("tvdb-artwork", &url) {
            return supported_artwork(data);
        }
        if self.cancellation.is_cancelled() {
            return Err("TVDB artwork lookup cancelled".to_owned());
        }
        let response = ureq::get(&url)
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
            return Err("TVDB artwork was empty or exceeded 10 MiB".to_owned());
        }
        let artwork = supported_artwork(data)?;
        self.cache.write_bytes("tvdb-artwork", &url, &artwork.data);
        Ok(artwork)
    }

    fn exact_search_results(
        &self,
        query: &str,
        remote_id: Option<&str>,
        media_type: Option<TvdbMediaType>,
    ) -> Result<Vec<SearchResult>, String> {
        let response = self.search_results(query, remote_id, media_type)?;
        let query_year = year_in(query);
        let query_name = normalize_title(&without_year(query));
        Ok(response
            .into_iter()
            .filter(|result| remote_id.is_some() || result.matches_title(&query_name))
            .filter(|result| query_year.is_none_or(|year| result.year() == Some(year)))
            .collect())
    }

    fn search_results(
        &self,
        query: &str,
        remote_id: Option<&str>,
        media_type: Option<TvdbMediaType>,
    ) -> Result<Vec<SearchResult>, String> {
        let mut query_params = vec![("limit", "20")];
        if !query.is_empty() {
            query_params.push(("query", query));
        }
        if let Some(remote_id) = remote_id {
            query_params.push(("remote_id", remote_id));
        }
        if let Some(media_type) = media_type {
            query_params.push(("type", media_type.search_type()));
        }
        let response: Envelope<Vec<SearchResult>> = self.get_json("/search", &query_params)?;
        Ok(response
            .data
            .into_iter()
            .filter(|result| matches!(result.media_type.as_str(), "movie" | "series"))
            .collect())
    }

    fn details_from_search(&self, result: SearchResult) -> Result<TvdbMatch, String> {
        let id = parse_id(&result.tvdb_id)?;
        let media_type = if result.media_type == "movie" {
            TvdbMediaType::Movie
        } else {
            TvdbMediaType::Tv
        };
        let path = match media_type {
            TvdbMediaType::Movie => format!("/movies/{id}"),
            TvdbMediaType::Tv => format!("/series/{id}"),
        };
        let details: Envelope<BaseRecord> = self.get_json(&path, &[])?;
        Ok(match_from_base_record(details.data, media_type))
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
            return diagnostics::from_value("TVDB", &cache_key, "cache", cached);
        }
        if self.cancellation.is_cancelled() {
            return Err("TVDB lookup cancelled".to_owned());
        }
        let token = self.token(false)?;
        let first = self.request_json(path, query, &token);
        let network_response = if first
            .as_ref()
            .err()
            .is_some_and(|error| matches!(error.as_ref(), ureq::Error::Status(401, _)))
        {
            let token = self.token(true)?;
            self.request_json(path, query, &token)
        } else {
            first
        };
        match network_response {
            Ok(value) => {
                self.cache.write(CACHE_NAMESPACE, &cache_key, &value);
                diagnostics::from_value("TVDB", &cache_key, "network", value)
            }
            Err(error) => {
                let stale = self
                    .cache
                    .read_stale::<serde_json::Value>(CACHE_NAMESPACE, &cache_key)
                    .ok_or_else(|| error.to_string())?;
                diagnostics::from_value("TVDB", &cache_key, "stale-cache", stale)
            }
        }
    }

    fn request_json(
        &self,
        path: &str,
        query: &[(&str, &str)],
        token: &str,
    ) -> Result<serde_json::Value, Box<ureq::Error>> {
        let mut request = ureq::get(&format!("{API_ROOT}{path}"))
            .set("Authorization", &format!("Bearer {token}"))
            .set(
                "User-Agent",
                concat!("MediaRip/", env!("CARGO_PKG_VERSION")),
            )
            .set("Accept", "application/json")
            .timeout(Duration::from_secs(10));
        for (key, value) in query {
            request = request.query(key, value);
        }
        let response = request.call().map_err(Box::new)?;
        response
            .into_json::<serde_json::Value>()
            .map_err(ureq::Error::from)
            .map_err(Box::new)
    }

    fn token(&self, force_login: bool) -> Result<String, String> {
        let key = format!(
            "{}:{}",
            self.api_key,
            self.subscriber_pin.unwrap_or_default()
        );
        if !force_login && let Some(token) = self.cache.read::<String>("tvdb-auth", &key) {
            return Ok(token);
        }
        if self.cancellation.is_cancelled() {
            return Err("TVDB login cancelled".to_owned());
        }
        let mut body = serde_json::json!({ "apikey": self.api_key });
        if let Some(pin) = self.subscriber_pin.filter(|pin| !pin.is_empty()) {
            body["pin"] = serde_json::Value::String(pin.to_owned());
        }
        let response = ureq::post(&format!("{API_ROOT}/login"))
            .set(
                "User-Agent",
                concat!("MediaRip/", env!("CARGO_PKG_VERSION")),
            )
            .set("Accept", "application/json")
            .timeout(Duration::from_secs(10))
            .send_json(body)
            .map_err(|error| error.to_string())?
            .into_json::<Envelope<LoginData>>()
            .map_err(|error| error.to_string())?;
        self.cache.write("tvdb-auth", &key, &response.data.token);
        Ok(response.data.token)
    }
}

fn match_from_base_record(details: BaseRecord, media_type: TvdbMediaType) -> TvdbMatch {
    TvdbMatch {
        id: details.id,
        media_type,
        title: details.name,
        year: details.year.as_deref().and_then(parse_year),
        runtime_minutes: details.runtime.or(details.average_runtime),
        artwork_url: details.image,
    }
}

impl SearchResult {
    fn titles(&self) -> impl Iterator<Item = &str> {
        [
            self.name.as_deref(),
            self.title.as_deref(),
            self.name_translated.as_deref(),
        ]
        .into_iter()
        .flatten()
        .chain(self.aliases.iter().map(String::as_str))
    }

    fn matches_title(&self, normalized_query: &str) -> bool {
        self.titles()
            .any(|title| normalize_title(title) == normalized_query)
    }

    fn title_review_score(&self, query: &str) -> Option<u8> {
        self.titles()
            .filter_map(|title| title_review_score(query, title))
            .max()
    }

    fn year(&self) -> Option<u32> {
        self.year.as_deref().and_then(parse_year)
    }
}

fn parse_id(value: &str) -> Result<u64, String> {
    value
        .trim_start_matches(|character: char| !character.is_ascii_digit())
        .parse()
        .map_err(|_| format!("TVDB returned invalid ID {value:?}"))
}

fn parse_year(value: &str) -> Option<u32> {
    value.get(..4)?.parse().ok()
}

fn normalize_title(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn year_in(value: &str) -> Option<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .find_map(|part| parse_year(part).filter(|year| (1888..=2100).contains(year)))
}

fn without_year(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|part| parse_year(part).is_none_or(|year| !(1888..=2100).contains(&year)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn supported_artwork(data: Vec<u8>) -> Result<CoverArt, String> {
    let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        return Err("TVDB returned unsupported artwork".to_owned());
    };
    Ok(CoverArt::new(mime, data))
}

#[cfg(test)]
mod tests {
    use super::{SearchResult, normalize_title, parse_id, year_in};

    #[test]
    fn normalizes_titles_and_parses_tvdb_identifiers() {
        assert_eq!(normalize_title("M*A*S*H"), "mash");
        assert_eq!(parse_id("series-70994").unwrap(), 70_994);
        assert_eq!(year_in("Example 2005 S2 D1"), Some(2005));
    }

    #[test]
    fn exact_matching_accepts_tvdb_aliases() {
        let result = SearchResult {
            aliases: vec!["Wall-E".into()],
            name: Some("WALL·E".into()),
            title: None,
            name_translated: None,
            tvdb_id: "movie-1".into(),
            media_type: "movie".into(),
            year: Some("2008".into()),
        };
        assert!(result.matches_title(&normalize_title("Wall-E")));
        assert_eq!(result.year(), Some(2008));
    }
}
