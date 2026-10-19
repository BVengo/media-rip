use media_rip_core::{CoverArt, ScanOptions, ports::CancellationToken};

use super::{
    cache::MetadataCache,
    tmdb::{MediaType, TmdbClient},
    tvdb::TvdbClient,
    tvmaze::TvmazeClient,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSearchKind {
    Movie,
    Television,
}

#[derive(Debug, Clone)]
pub struct VideoSearchCandidate {
    pub title: String,
    pub year: Option<u32>,
    pub kind: VideoSearchKind,
    pub provider: &'static str,
    pub metadata_url: String,
    pub artwork_path: Option<String>,
}

/// Return alternatives without requiring an exact title or a known release year.
pub fn search_video_metadata(
    query: &str,
    kind: VideoSearchKind,
    options: &ScanOptions,
    cancellation: &CancellationToken,
) -> Result<Vec<VideoSearchCandidate>, String> {
    search_with_cache(
        query,
        kind,
        options,
        cancellation,
        &MetadataCache::platform_default(),
    )
}

fn search_with_cache(
    query: &str,
    kind: VideoSearchKind,
    options: &ScanOptions,
    cancellation: &CancellationToken,
    cache: &MetadataCache,
) -> Result<Vec<VideoSearchCandidate>, String> {
    if !options.allow_online_access {
        return Err("Enable online metadata lookups in Settings before searching.".to_owned());
    }
    let query = query.trim();
    if query.is_empty() {
        return Err("Enter a title to search for.".to_owned());
    }
    if cancellation.is_cancelled() {
        return Err("Metadata search cancelled.".to_owned());
    }
    let mut errors = Vec::new();
    let mut searched = false;
    if let Some(token) = options
        .tmdb_access_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        searched = true;
        let media_type = match kind {
            VideoSearchKind::Movie => MediaType::Movie,
            VideoSearchKind::Television => MediaType::Tv,
        };
        match TmdbClient::new(token, cache, cancellation).search_choices(query, media_type) {
            Ok(matches) if !matches.is_empty() => {
                return Ok(matches
                    .into_iter()
                    .take(20)
                    .map(|matched| VideoSearchCandidate {
                        metadata_url: format!(
                            "https://www.themoviedb.org/{}/{}",
                            media_type.as_str(),
                            matched.id
                        ),
                        title: matched.title,
                        year: matched.year,
                        kind,
                        provider: "TMDB",
                        artwork_path: matched.poster_path,
                    })
                    .collect());
            }
            Ok(_) => {}
            Err(error) => errors.push(format!("TMDB: {error}")),
        }
    }
    if let Some(key) = options
        .tvdb_api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        searched = true;
        match TvdbClient::new(
            key,
            options.tvdb_subscriber_pin.as_deref(),
            cache,
            cancellation,
        )
        .search_choices(query, kind)
        {
            Ok(matches) if !matches.is_empty() => {
                return Ok(matches.into_iter().take(20).collect());
            }
            Ok(_) => {}
            Err(error) => errors.push(format!("TVDB: {error}")),
        }
    }
    if kind == VideoSearchKind::Television && options.tvmaze_enabled {
        searched = true;
        match TvmazeClient::new(cache, cancellation).search_choices(query) {
            Ok(matches) => {
                return Ok(matches
                    .into_iter()
                    .take(20)
                    .map(|matched| VideoSearchCandidate {
                        metadata_url: format!("https://www.tvmaze.com/shows/{}", matched.id),
                        title: matched.title,
                        year: matched.year,
                        kind,
                        provider: "TVMaze",
                        artwork_path: matched.artwork_url,
                    })
                    .collect());
            }
            Err(error) => errors.push(format!("TVMaze: {error}")),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    if !searched {
        return Err(match kind {
            VideoSearchKind::Movie => "Add a TMDB read access token or TVDB API key in Settings → Integrations to search movies.",
            VideoSearchKind::Television => "Enable TVMaze or configure TMDB or TVDB in Settings → Integrations to search TV series.",
        }.to_owned());
    }
    Ok(Vec::new())
}

/// Load artwork separately so search results can be shown immediately.
pub fn load_video_search_artwork(
    candidate: &VideoSearchCandidate,
    options: &ScanOptions,
    cancellation: &CancellationToken,
) -> Option<CoverArt> {
    if !options.allow_online_access || cancellation.is_cancelled() {
        return None;
    }
    let path = candidate.artwork_path.as_deref()?;
    let cache = MetadataCache::platform_default();
    match candidate.provider {
        "TMDB" => TmdbClient::new(options.tmdb_access_token.as_deref()?, &cache, cancellation)
            .artwork(path)
            .ok(),
        "TVDB" => TvdbClient::new(
            options.tvdb_api_key.as_deref()?,
            options.tvdb_subscriber_pin.as_deref(),
            &cache,
            cancellation,
        )
        .artwork(path)
        .ok(),
        "TVMaze" => TvmazeClient::new(&cache, cancellation).artwork(path).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movie_choices_preserve_ambiguous_titles_and_provider_ids_without_a_year() {
        let root =
            std::env::temp_dir().join(format!("mediarip-search-movie-{}", std::process::id()));
        let cache = MetadataCache::at(root.clone());
        let params = [
            ("query", "Dune"),
            ("include_adult", "false"),
            ("language", "en-US"),
        ];
        cache.write("tmdb-raw-v1", &format!("/search/movie?{}", serde_json::to_string(&params).unwrap()), &serde_json::json!({
            "results": [
                {"id": 841, "title": "Dune", "release_date": "1984-12-14", "poster_path": "/old.jpg"},
                {"id": 438631, "title": "Dune", "release_date": "2021-09-15"},
                {"id": 123, "title": "Dune: Part Two", "release_date": ""}
            ]
        }));
        let options = ScanOptions {
            tmdb_access_token: Some("test-token".to_owned()),
            ..Default::default()
        };
        let matches = search_with_cache(
            " Dune ",
            VideoSearchKind::Movie,
            &options,
            &CancellationToken::default(),
            &cache,
        )
        .unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].year, Some(1984));
        assert_eq!(matches[1].year, Some(2021));
        assert_eq!(matches[2].year, None);
        assert_eq!(
            matches[1].metadata_url,
            "https://www.themoviedb.org/movie/438631"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tv_choices_allow_fuzzy_matches_without_credentials() {
        let root = std::env::temp_dir().join(format!("mediarip-search-tv-{}", std::process::id()));
        let cache = MetadataCache::at(root.clone());
        cache.write(
            "tvmaze-raw-v1",
            &format!(
                "/search/shows?{}",
                serde_json::to_string(&[("q", "Office")]).unwrap()
            ),
            &serde_json::json!([
                {"show": {"id": 526, "name": "The Office", "premiered": "2001-07-09"}},
                {"show": {"id": 6, "name": "The Office", "premiered": "2005-03-24"}}
            ]),
        );
        let matches = search_with_cache(
            "Office",
            VideoSearchKind::Television,
            &ScanOptions::default(),
            &CancellationToken::default(),
            &cache,
        )
        .unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[1].year, Some(2005));
        assert_eq!(matches[1].metadata_url, "https://www.tvmaze.com/shows/6");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn movie_search_uses_tvdb_when_it_is_the_configured_provider() {
        let root =
            std::env::temp_dir().join(format!("mediarip-search-tvdb-{}", std::process::id()));
        let cache = MetadataCache::at(root.clone());
        let params = [("limit", "20"), ("query", "Dune"), ("type", "movie")];
        cache.write("tvdb-raw-v1", &format!("/search?{}", serde_json::to_string(&params).unwrap()), &serde_json::json!({
            "data": [
                {"tvdb_id": "1", "name": "Dune", "type": "movie", "slug": "dune-1984", "year": "1984"},
                {"tvdb_id": "2", "name": "Dune", "type": "movie", "slug": "dune-2021", "year": "2021"},
                {"tvdb_id": "3", "name": "Dune", "type": "series", "slug": "dune", "year": "2000"}
            ]
        }));
        let options = ScanOptions {
            tvdb_api_key: Some("test-key".to_owned()),
            ..Default::default()
        };
        let matches = search_with_cache(
            "Dune",
            VideoSearchKind::Movie,
            &options,
            &CancellationToken::default(),
            &cache,
        )
        .unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches[1].metadata_url,
            "https://thetvdb.com/movies/dune-2021"
        );
        assert_eq!(matches[1].year, Some(2021));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn search_reports_disabled_access_missing_provider_and_empty_query() {
        let token = CancellationToken::default();
        let options = ScanOptions::default();
        assert!(
            search_video_metadata("Dune", VideoSearchKind::Movie, &options, &token)
                .unwrap_err()
                .contains("TMDB read access token")
        );
        assert!(
            search_video_metadata(" ", VideoSearchKind::Television, &options, &token)
                .unwrap_err()
                .contains("Enter a title")
        );
        let offline = ScanOptions {
            allow_online_access: false,
            ..options
        };
        assert!(
            search_video_metadata("Dune", VideoSearchKind::Movie, &offline, &token)
                .unwrap_err()
                .contains("Enable online")
        );
    }
}
