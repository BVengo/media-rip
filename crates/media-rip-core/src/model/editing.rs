//! Catalog editing rules shared by interfaces. No GTK or provider requests.
use crate::{
    CoverArt, DiscKind, MediaCatalog, Title, TrackCoverArt,
    VideoEpisodeContext as EpisodeMetadataContext, VideoMetadataSuggestion,
    util::{ensure_extension, sanitize_file_name},
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestionShift {
    pub offset: i32,
    pub unmatched: usize,
    pub first: u32,
    pub last: u32,
}
pub fn format_audio_cd_file_name(template: &str, title: &crate::Title, album: &str) -> String {
    let template = if template.trim().is_empty() {
        "{artist}/{album}/{disc}-{track:02} - {title}.flac"
    } else {
        template
    };
    let tag = |key: &str| {
        title
            .tags
            .iter()
            .find(|tag| tag.key.eq_ignore_ascii_case(key))
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

pub fn set_tag(
    catalog: &mut crate::MediaCatalog,
    title_id: u32,
    key: &str,
    value: &str,
    all_tracks: bool,
    file_name_template: &str,
    output_extension: &str,
) -> bool {
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return false;
    }
    let album = catalog.source.name.clone();
    let is_audio_cd = catalog.source.kind == DiscKind::AudioCd;
    let mut renamed = false;
    for title in &mut catalog.titles {
        if title.id != title_id && !all_tracks {
            continue;
        }
        let had_auto_name = is_audio_cd
            && title.output_file
                == ensure_extension(
                    &format_audio_cd_file_name(file_name_template, title, &album),
                    output_extension,
                );
        let mut updated = false;
        title.tags.retain_mut(|tag| {
            if !tag.key.eq_ignore_ascii_case(&key) {
                return true;
            }
            if updated {
                return false;
            }
            tag.key.clone_from(&key);
            tag.value = value.to_owned();
            updated = true;
            true
        });
        if !updated {
            title.tags.push(crate::MediaTag::new(&key, value));
        }
        if key == "title" {
            title.name = value.to_owned();
        }
        if key == "date"
            && let Some(metadata) = &mut title.video_metadata
        {
            metadata.year = value
                .split('-')
                .next()
                .and_then(|year| year.trim().parse::<u32>().ok())
                .filter(|year| (1..=9999).contains(year));
        }
        if key == "artist"
            && is_audio_cd
            && let Some(track) = title.tracks.first_mut()
        {
            track.name = (!value.trim().is_empty()).then(|| value.to_owned());
        }
        if had_auto_name {
            let updated_output_file = ensure_extension(
                &format_audio_cd_file_name(file_name_template, title, &album),
                output_extension,
            );
            if title.output_file != updated_output_file {
                title.output_file = updated_output_file;
                renamed = true;
            }
        }
    }
    if all_tracks && key == "album" {
        catalog.source.name = value.to_owned();
    }
    renamed
}

pub fn rename_title(catalog: &mut crate::MediaCatalog, title_id: u32, name: &str) -> bool {
    let Some(title) = catalog.titles.iter_mut().find(|title| title.id == title_id) else {
        return false;
    };
    title.name = name.to_owned();
    if let Some(tag) = title
        .tags
        .iter_mut()
        .find(|tag| tag.key.eq_ignore_ascii_case("title"))
    {
        tag.value = name.to_owned();
    } else {
        title.tags.push(crate::MediaTag::new("title", name));
    }
    true
}

pub fn rename_track(
    catalog: &mut crate::MediaCatalog,
    title_id: u32,
    stream_index: u32,
    name: String,
) -> Option<crate::Track> {
    let track = catalog
        .titles
        .iter_mut()
        .find(|title| title.id == title_id)?
        .tracks
        .iter_mut()
        .find(|track| track.index == stream_index)?;
    track.name = (!name.trim().is_empty()).then_some(name);
    Some(track.clone())
}

fn clear_video_identity_tags(tags: &mut Vec<crate::MediaTag>) {
    tags.retain(|tag| {
        !matches!(
            tag.key.to_ascii_lowercase().as_str(),
            "title"
                | "show"
                | "date"
                | "season_number"
                | "episode_id"
                | "tmdb_id"
                | "tvdb_id"
                | "tvmaze_id"
        )
    });
}

pub fn apply_video_suggestion(
    catalog: &mut crate::MediaCatalog,
    title_id: u32,
    output_extension: &str,
) -> bool {
    let Some(position) = catalog
        .video_metadata_suggestions
        .iter()
        .position(|suggestion| suggestion.title_id == title_id)
    else {
        return false;
    };
    let Some(title_index) = catalog.titles.iter().position(|title| title.id == title_id) else {
        return false;
    };
    let suggestion = catalog.video_metadata_suggestions.remove(position);
    let mut output_file = ensure_extension(&sanitize_file_name(&suggestion.name), output_extension);
    if catalog.titles.iter().enumerate().any(|(index, title)| {
        index != title_index && title.output_file.eq_ignore_ascii_case(&output_file)
    }) {
        output_file = ensure_extension(
            &format!(
                "{}_{}",
                sanitize_file_name(&suggestion.name),
                sanitize_file_name(&catalog.titles[title_index].source_title_id)
            ),
            output_extension,
        );
    }
    let title = &mut catalog.titles[title_index];
    title.name = suggestion.name;
    title.output_file = output_file;
    title.video_metadata = Some(suggestion.metadata);
    clear_video_identity_tags(&mut title.tags);
    for suggested_tag in suggestion.tags {
        if let Some(tag) = title
            .tags
            .iter_mut()
            .find(|tag| tag.key.eq_ignore_ascii_case(&suggested_tag.key))
        {
            *tag = suggested_tag;
        } else {
            title.tags.push(suggested_tag);
        }
    }
    update_probable_suggestion_count(catalog);
    true
}

pub fn preserve_choices_and_stage_reidentification(
    catalog: &mut crate::MediaCatalog,
    previous: &crate::MediaCatalog,
) {
    let mut staged = Vec::new();
    for title in &mut catalog.titles {
        let Some(old_title) = previous
            .titles
            .iter()
            .find(|candidate| candidate.source_title_id == title.source_title_id)
        else {
            continue;
        };

        title.selected = old_title.selected;
        for track in &mut title.tracks {
            if let Some(old_track) = old_title
                .tracks
                .iter()
                .find(|candidate| candidate.index == track.index)
            {
                track.selected = old_track.selected;
                track.name = old_track.name.clone();
            }
        }

        if let Some(proposed_metadata) = title.video_metadata.take()
            && (old_title.video_metadata.as_ref() != Some(&proposed_metadata)
                || old_title.name != title.name)
        {
            staged.push(VideoMetadataSuggestion {
                title_id: title.id,
                name: title.name.clone(),
                tags: title.tags.clone(),
                metadata: proposed_metadata,
                evidence: vec!["Matched from the identification you requested".to_owned()],
                assumptions: vec![
                    "The disc title mapping is unchanged from the current source scan".to_owned(),
                ],
                episode_choices: Vec::new(),
            });
        }

        title.name = old_title.name.clone();
        title.output_file = old_title.output_file.clone();
        title.tags = old_title.tags.clone();
        title.video_metadata = old_title.video_metadata.clone();
    }
    for suggestion in staged {
        if let Some(existing) = catalog
            .video_metadata_suggestions
            .iter_mut()
            .find(|candidate| candidate.title_id == suggestion.title_id)
        {
            *existing = suggestion;
        } else {
            catalog.video_metadata_suggestions.push(suggestion);
        }
    }
    update_probable_suggestion_count(catalog);
}

pub fn assign_video_episode(
    catalog: &mut crate::MediaCatalog,
    context: &EpisodeMetadataContext,
    title_id: u32,
    episode: u32,
    output_extension: &str,
) -> bool {
    let Some(choice) = context
        .choices
        .iter()
        .find(|choice| choice.episode == episode)
        .cloned()
    else {
        return false;
    };
    let Some(title_index) = catalog.titles.iter().position(|title| title.id == title_id) else {
        return false;
    };
    let mut output_file = ensure_extension(&sanitize_file_name(&choice.name), output_extension);
    if catalog.titles.iter().enumerate().any(|(index, title)| {
        index != title_index && title.output_file.eq_ignore_ascii_case(&output_file)
    }) {
        output_file = ensure_extension(
            &format!(
                "{}_{}",
                sanitize_file_name(&choice.name),
                sanitize_file_name(&catalog.titles[title_index].source_title_id)
            ),
            output_extension,
        );
    }
    let mut metadata = context.metadata.clone();
    metadata.episode = Some(episode);
    metadata.content_type = Some("Episode".to_owned());
    metadata.attribution.confidence_percent = 100;
    metadata.attribution.match_method =
        "User-selected episode from matched season metadata".to_owned();

    let title = &mut catalog.titles[title_index];
    title.name = choice.name;
    title.output_file = output_file;
    title.video_metadata = Some(metadata);
    clear_video_identity_tags(&mut title.tags);
    for suggested_tag in choice.tags {
        if let Some(tag) = title
            .tags
            .iter_mut()
            .find(|tag| tag.key.eq_ignore_ascii_case(&suggested_tag.key))
        {
            *tag = suggested_tag;
        } else {
            title.tags.push(suggested_tag);
        }
    }
    catalog
        .video_metadata_suggestions
        .retain(|suggestion| suggestion.title_id != title_id);
    update_probable_suggestion_count(catalog);
    true
}

pub fn update_probable_suggestion_count(catalog: &mut crate::MediaCatalog) {
    let remaining = catalog.video_metadata_suggestions.len();
    if let Some(field) = catalog
        .source_info
        .iter_mut()
        .find(|field| field.label == "Probable episode matches")
    {
        field.value =
            format!("{remaining} suggestion(s), not applied — review them in the content list");
    }
    if remaining == 0 {
        catalog
            .source_info
            .retain(|field| field.label != "Probable episode matches");
    }
}

pub fn episode_metadata_context(catalog: &crate::MediaCatalog) -> Option<EpisodeMetadataContext> {
    if let Some(context) = &catalog.video_episode_context {
        return Some(context.clone());
    }
    let suggestion = catalog
        .video_metadata_suggestions
        .iter()
        .find(|suggestion| !suggestion.episode_choices.is_empty())?;
    let mut metadata = suggestion.metadata.clone();
    metadata.episode = None;
    Some(EpisodeMetadataContext {
        metadata,
        choices: suggestion.episode_choices.clone(),
    })
}

pub fn shift_video_suggestions(catalog: &mut crate::MediaCatalog, offset: i32) -> bool {
    shift_video_suggestions_with_result(catalog, offset).is_ok_and(|result| result.is_some())
}

pub fn shift_video_suggestions_with_result(
    catalog: &mut crate::MediaCatalog,
    offset: i32,
) -> Result<Option<SuggestionShift>, String> {
    if offset == 0 {
        return Ok(None);
    }
    let updates = catalog
        .video_metadata_suggestions
        .iter()
        .map(|suggestion| {
            let episode = suggestion
                .metadata
                .episode
                .and_then(|episode| episode.checked_add_signed(offset))?;
            suggestion
                .episode_choices
                .iter()
                .find(|choice| choice.episode == episode)
                .map(|choice| (episode, choice.name.clone(), choice.tags.clone()))
        })
        .collect::<Vec<_>>();
    let episodic_count = catalog
        .video_metadata_suggestions
        .iter()
        .filter(|suggestion| suggestion.metadata.episode.is_some())
        .count();
    let unmatched = catalog
        .video_metadata_suggestions
        .iter()
        .zip(&updates)
        .filter(|(suggestion, update)| suggestion.metadata.episode.is_some() && update.is_none())
        .count();
    if unmatched == episodic_count {
        return Err(
            "That starting episode leaves no available season episodes for these disc titles."
                .to_owned(),
        );
    }
    let mut shifted = Vec::with_capacity(updates.len() - unmatched);
    for (mut suggestion, update) in catalog.video_metadata_suggestions.drain(..).zip(updates) {
        if suggestion.metadata.episode.is_none() {
            shifted.push(suggestion);
            continue;
        }
        let Some((episode, name, tags)) = update else {
            continue;
        };
        suggestion.metadata.episode = Some(episode);
        suggestion.name = name;
        suggestion.tags = tags;
        shifted.push(suggestion);
    }
    let first = shifted
        .iter()
        .filter_map(|suggestion| suggestion.metadata.episode)
        .min()
        .expect("at least one shifted suggestion remains");
    let last = shifted
        .iter()
        .filter_map(|suggestion| suggestion.metadata.episode)
        .max()
        .expect("at least one shifted suggestion remains");
    catalog.video_metadata_suggestions = shifted;
    update_probable_suggestion_count(catalog);
    Ok(Some(SuggestionShift {
        offset,
        unmatched,
        first,
        last,
    }))
}

pub fn set_suggestion_first_episode(
    catalog: &mut crate::MediaCatalog,
    requested_first: u32,
) -> Result<Option<SuggestionShift>, String> {
    let current_first = catalog
        .video_metadata_suggestions
        .iter()
        .filter_map(|suggestion| suggestion.metadata.episode)
        .min()
        .ok_or_else(|| "No episode suggestions are available to shift.".to_owned())?;
    let difference = i64::from(requested_first) - i64::from(current_first);
    let offset = i32::try_from(difference)
        .map_err(|_| "The requested episode number is outside the supported range.".to_owned())?;
    shift_video_suggestions_with_result(catalog, offset)
}

pub fn shift_video_suggestion_seasons(catalog: &mut crate::MediaCatalog, offset: i32) -> bool {
    let seasons = catalog
        .video_metadata_suggestions
        .iter()
        .filter(|suggestion| suggestion.metadata.season.is_some())
        .map(|suggestion| suggestion.metadata.season?.checked_add_signed(offset))
        .collect::<Option<Vec<_>>>();
    let Some(seasons) = seasons else {
        return false;
    };
    for (suggestion, season) in catalog
        .video_metadata_suggestions
        .iter_mut()
        .filter(|suggestion| suggestion.metadata.season.is_some())
        .zip(seasons)
    {
        let previous = suggestion.metadata.season.replace(season).unwrap_or(season);
        update_suggestion_season_text(&mut suggestion.name, previous, season);
        update_season_tags(&mut suggestion.tags, season);
        for choice in &mut suggestion.episode_choices {
            update_suggestion_season_text(&mut choice.name, previous, season);
            update_season_tags(&mut choice.tags, season);
        }
    }
    true
}

pub fn replace_suggestion_season_choices(
    catalog: &mut crate::MediaCatalog,
    requested_season: u32,
    choices: &[crate::VideoEpisodeChoice],
) -> Option<i32> {
    let current_season = catalog
        .video_metadata_suggestions
        .iter()
        .find(|suggestion| suggestion.metadata.season.is_some())?
        .metadata
        .season?;
    let difference = i64::from(requested_season) - i64::from(current_season);
    let offset = i32::try_from(difference).ok()?;
    let selected = catalog
        .video_metadata_suggestions
        .iter()
        .filter(|suggestion| suggestion.metadata.season.is_some())
        .map(|suggestion| {
            let episode = suggestion.metadata.episode?;
            choices
                .iter()
                .find(|choice| choice.episode == episode)
                .cloned()
        })
        .collect::<Option<Vec<_>>>()?;

    for (suggestion, selected) in catalog
        .video_metadata_suggestions
        .iter_mut()
        .filter(|suggestion| suggestion.metadata.season.is_some())
        .zip(selected)
    {
        suggestion.metadata.season = Some(requested_season);
        suggestion.name = selected.name;
        suggestion.tags = selected.tags;
        suggestion.episode_choices = choices.to_vec();
    }
    (offset != 0).then_some(offset)
}

pub fn update_suggestion_season_text(value: &mut String, previous: u32, season: u32) {
    *value = value.replacen(&format!("S{previous:02}E"), &format!("S{season:02}E"), 1);
}

pub fn update_season_tags(tags: &mut [crate::MediaTag], season: u32) {
    for tag in tags {
        if tag.key.eq_ignore_ascii_case("season_number") {
            tag.value = season.to_string();
        }
    }
}

pub fn set_cover_art(
    catalog: &mut crate::MediaCatalog,
    title_id: Option<u32>,
    cover_art: CoverArt,
) {
    if let Some(title_id) = title_id {
        if catalog.cover_art.as_ref() == Some(&cover_art) {
            catalog.track_cover_art.remove(&title_id);
        } else {
            catalog
                .track_cover_art
                .insert(title_id, TrackCoverArt::Custom(cover_art));
        }
    } else {
        catalog.track_cover_art.retain(|_, track_art| {
            !matches!(track_art, TrackCoverArt::Custom(custom) if custom == &cover_art)
        });
        catalog.cover_art = Some(cover_art);
    }
}

pub fn remove_cover_art(catalog: &mut crate::MediaCatalog, title_id: Option<u32>) {
    if let Some(title_id) = title_id {
        catalog
            .track_cover_art
            .insert(title_id, TrackCoverArt::Removed);
    } else {
        catalog.cover_art = None;
    }
}

pub fn mark_title_completed(catalog: &mut crate::MediaCatalog, title_id: u32) -> bool {
    let Some(title) = catalog.titles.iter_mut().find(|title| title.id == title_id) else {
        return false;
    };
    title.selected = false;
    for track in &mut title.tracks {
        track.selected = false;
    }
    true
}

pub fn set_catalog_selection(catalog: &mut crate::MediaCatalog, selected: bool) {
    for title in &mut catalog.titles {
        title.selected = selected;
        for track in &mut title.tracks {
            track.selected = selected;
        }
    }
}

pub fn episode_assignment_title_ids(catalog: &MediaCatalog) -> BTreeSet<u32> {
    const MIN_EPISODE_MS: u64 = 20 * 60_000;
    const MAX_EPISODE_MS: u64 = 120 * 60_000;
    const PLAY_ALL_TOLERANCE_MS: u64 = 3_000;

    let episode_like = |title: &Title| {
        (MIN_EPISODE_MS..=MAX_EPISODE_MS).contains(&title.duration_ms) && title.chapters.len() >= 2
    };
    let mut play_all_ids = BTreeSet::new();
    for (play_all_index, play_all) in catalog.titles.iter().enumerate() {
        let mut episode_count = 0;
        let mut total_duration_ms = 0_u64;
        for title in catalog.titles.iter().skip(play_all_index + 1) {
            if !episode_like(title) {
                if episode_count > 0 {
                    break;
                }
                continue;
            }
            episode_count += 1;
            total_duration_ms = total_duration_ms.saturating_add(title.duration_ms);
            if episode_count >= 2
                && total_duration_ms.abs_diff(play_all.duration_ms) <= PLAY_ALL_TOLERANCE_MS
            {
                play_all_ids.insert(play_all.id);
                break;
            }
            if total_duration_ms > play_all.duration_ms.saturating_add(PLAY_ALL_TOLERANCE_MS)
                || episode_count == 8
            {
                break;
            }
        }
    }

    catalog
        .titles
        .iter()
        .filter(|title| episode_like(title) && !play_all_ids.contains(&title.id))
        .map(|title| title.id)
        .collect()
}
