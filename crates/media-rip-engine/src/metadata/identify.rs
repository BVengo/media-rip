use media_rip_core::{DiscKind, InfoField, MediaCatalog, ScanOptions, ports::CancellationToken};

use super::{
    DiscHint, MediaType, apply_movie_match, cache::MetadataCache, disc_hint_with_link,
    dvd_metadata_title_indices, parallel_provider_lookups, parse_disc_label,
    parse_video_metadata_link, probable_episode_suggestions, reconcile_provider_lookups,
    video_episode_context,
};

/// Identify the loaded titles without opening the source or probing its streams again.
pub fn identify_loaded_catalog(
    catalog: MediaCatalog,
    options: &ScanOptions,
    cancellation: &CancellationToken,
) -> Result<MediaCatalog, String> {
    identify_with_cache(
        catalog,
        options,
        cancellation,
        &MetadataCache::platform_default(),
    )
}

fn identify_with_cache(
    mut catalog: MediaCatalog,
    options: &ScanOptions,
    cancellation: &CancellationToken,
    cache: &MetadataCache,
) -> Result<MediaCatalog, String> {
    if cancellation.is_cancelled() {
        return Err("Metadata identification cancelled.".to_owned());
    }
    if !options.allow_online_access {
        return Err(
            "Enable online metadata lookups in Settings before identifying a title.".to_owned(),
        );
    }
    let link =
        parse_video_metadata_link(options.video_metadata_url.as_deref().unwrap_or_default())?;
    let parsed_label = parse_disc_label(&catalog.source.name);
    let hint = disc_hint_with_link(&parsed_label, Some(&link)).or_else(|| {
        link.is_series()
            .then(|| {
                catalog
                    .video_episode_context
                    .as_ref()?
                    .metadata
                    .season
                    .map(|season| DiscHint {
                        season,
                        volume: None,
                        disc: 1,
                    })
            })
            .flatten()
    });
    let (tmdb, tvdb, tvmaze) = parallel_provider_lookups(
        options,
        cache,
        cancellation,
        &catalog.source.name,
        hint,
        None,
        Some(&link),
    );
    if cancellation.is_cancelled() {
        return Err("Metadata identification cancelled.".to_owned());
    }
    let (resolved, artwork, attributions, warnings) =
        reconcile_provider_lookups(tmdb, tvdb, tvmaze, true);
    let resolved = resolved.ok_or_else(|| {
        if warnings.is_empty() {
            "No metadata was found for the selected identity.".to_owned()
        } else {
            warnings.join("\n")
        }
    })?;
    let mut eligible = if catalog.source.kind == DiscKind::Dvd {
        dvd_metadata_title_indices(&catalog.titles)
            .into_iter()
            .map(|index| catalog.titles[index].clone())
            .collect()
    } else {
        catalog.titles.clone()
    };
    catalog.video_metadata_suggestions.clear();
    catalog.video_episode_context = None;
    match resolved.matched.media_type {
        MediaType::Movie => {
            // Keep the current title names and tags until the proposal is accepted.
            if let Some(suggestion) = apply_movie_match(&mut eligible, &resolved.matched, false) {
                catalog.video_metadata_suggestions.push(suggestion);
            }
        }
        MediaType::Tv => {
            catalog.video_episode_context = Some(video_episode_context(
                &resolved.matched,
                &resolved.episodes,
                hint.map(|hint| hint.season),
                98,
                resolved.match_method,
            ));
            if let Some(hint) = hint
                && !resolved.episodes.is_empty()
            {
                catalog.video_metadata_suggestions = probable_episode_suggestions(
                    &eligible,
                    &resolved.matched,
                    &resolved.episodes,
                    hint,
                )
                .suggestions;
            }
        }
    }
    catalog.source.name = resolved.matched.title;
    catalog.cover_art = artwork;
    catalog.metadata_attributions = attributions;
    set_identity_field(
        &mut catalog,
        "Metadata URL",
        options.video_metadata_url.clone(),
    );
    set_identity_field(
        &mut catalog,
        "Year",
        resolved.matched.year.map(|year| year.to_string()),
    );
    set_identity_field(
        &mut catalog,
        "Content",
        Some(if link.is_series() { "Series" } else { "Movie" }.to_owned()),
    );
    set_identity_field(&mut catalog, "Tag source", Some(link.provider().to_owned()));
    media_rip_core::model::editing::update_probable_suggestion_count(&mut catalog);
    Ok(catalog)
}

fn set_identity_field(catalog: &mut MediaCatalog, label: &str, value: Option<String>) {
    catalog.source_info.retain(|field| field.label != label);
    if let Some(value) = value {
        catalog.source_info.push(InfoField::new(label, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_catalog() -> MediaCatalog {
        serde_json::from_value(serde_json::json!({
            "source": {"id": "disc", "name": "UNMATCHED_DISC", "path": "/nonexistent/media-rip-disc", "kind": "Dvd"},
            "titles": [{
                "id": 7, "source_title_id": "8", "disc_title_number": 8,
                "name": "My title", "duration_ms": 8220000, "size_bytes": 4000000000_u64,
                "chapters": [{"index": 0, "start_ms": 0, "end_ms": 8220000}],
                "tracks": [{"index": 0, "kind": "Video", "codec": "mpeg2video", "properties": [], "selected": false}],
                "tags": [{"key": "comment", "value": "My notes"}],
                "output_file": "my-custom-filename.mkv", "selected": true
            }],
            "source_info": [{"label": "Disc content hash", "value": "existing-hash"}, {"label": "Year", "value": "2000"}]
        })).unwrap()
    }

    #[test]
    fn identifying_a_loaded_movie_preserves_content_without_accessing_the_disc() {
        let original = loaded_catalog();
        assert!(!original.source.path.exists());
        let root =
            std::env::temp_dir().join(format!("mediarip-loaded-movie-{}", std::process::id()));
        let cache = MetadataCache::at(root.clone());
        cache.write(
            "tmdb-raw-v1",
            "/movie/841?[]",
            &serde_json::json!({
                "id": 841, "title": "Dune", "release_date": "1984-12-14", "runtime": 137
            }),
        );
        let options = ScanOptions {
            tmdb_access_token: Some("test-token".to_owned()),
            video_metadata_url: Some("https://www.themoviedb.org/movie/841".to_owned()),
            ..Default::default()
        };
        let result = identify_with_cache(
            original.clone(),
            &options,
            &CancellationToken::default(),
            &cache,
        )
        .unwrap();
        assert_eq!(result.titles, original.titles);
        assert_eq!(result.source.path, original.source.path);
        assert_eq!(result.source.name, "Dune");
        assert_eq!(result.video_metadata_suggestions.len(), 1);
        assert_eq!(result.video_metadata_suggestions[0].title_id, 7);
        assert_eq!(
            result.video_metadata_suggestions[0].metadata.tmdb_id,
            Some(841)
        );
        assert!(result.source_info.contains(&InfoField::new("Year", "1984")));
        assert!(
            result
                .source_info
                .contains(&InfoField::new("Disc content hash", "existing-hash"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_or_disabled_identification_does_not_load_metadata() {
        let token = CancellationToken::default();
        token.cancel();
        assert!(
            identify_loaded_catalog(loaded_catalog(), &ScanOptions::default(), &token)
                .unwrap_err()
                .contains("cancelled")
        );
        let options = ScanOptions {
            allow_online_access: false,
            ..Default::default()
        };
        assert!(
            identify_loaded_catalog(loaded_catalog(), &options, &CancellationToken::default())
                .unwrap_err()
                .contains("Enable online")
        );
    }
}
