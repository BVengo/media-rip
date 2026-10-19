//! Media-specific editing context derived from observed streams and metadata.
use super::{DiscKind, MediaCatalog, StreamKind, Title, VideoMetadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataKind {
    Video,
    Extras,
    Film,
    Television,
    Music,
    Audiobook,
}

/// Structural hints choose an editor, never a provider identity or episode number.
/// Ignore repeated paths and suspect DVD layouts when comparing durations.
pub fn inferred_video_kind(titles: &[Title], title: &Title) -> Option<MetadataKind> {
    if title
        .dvd
        .as_ref()
        .is_some_and(|dvd| !dvd.warnings.is_empty())
    {
        return None;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut distinct = titles
        .iter()
        .filter(|item| {
            item.duration_ms > 0
                && item.dvd.as_ref().is_none_or(|dvd| dvd.warnings.is_empty())
                && item
                    .duplicate_key
                    .as_ref()
                    .is_none_or(|key| seen.insert(key))
        })
        .collect::<Vec<_>>();
    distinct.sort_by_key(|item| std::cmp::Reverse(item.duration_ms));
    if let Some(feature) = distinct.first() {
        let remaining = distinct
            .iter()
            .skip(1)
            .fold(0_u64, |sum, item| sum.saturating_add(item.duration_ms));
        let runner_up = distinct.get(1).map_or(0, |item| item.duration_ms);
        if feature.duration_ms >= 60 * 60_000
            && feature.duration_ms >= runner_up.saturating_mul(2)
            // Allow authoring gaps in play-all compilations.
            && feature.duration_ms > remaining.saturating_add(3 * 60_000)
            && (feature.id == title.id
                || (feature.duplicate_key.is_some()
                    && feature.duplicate_key == title.duplicate_key))
        {
            return Some(MetadataKind::Film);
        }
    }
    // Similar episode-length paths suggest television. A play-all title cannot
    // satisfy this range merely by containing the complete episode collection.
    if (15 * 60_000..=75 * 60_000).contains(&title.duration_ms) {
        let comparable = distinct
            .iter()
            .filter(|item| {
                (15 * 60_000..=75 * 60_000).contains(&item.duration_ms)
                    && item.duration_ms.abs_diff(title.duration_ms) <= title.duration_ms / 5
            })
            .count();
        if comparable >= 2 {
            return Some(MetadataKind::Television);
        }
    }
    None
}

impl MetadataKind {
    pub fn for_title(catalog: &MediaCatalog, title: &Title) -> Self {
        let audio = catalog.source.kind == DiscKind::AudioCd
            || (title
                .tracks
                .iter()
                .any(|track| track.kind == StreamKind::Audio)
                && !title
                    .tracks
                    .iter()
                    .any(|track| track.kind == StreamKind::Video));
        if audio {
            if let Some(kind) = title.tags.iter().find_map(|tag| {
                if !tag.key.eq_ignore_ascii_case("media_type") {
                    return None;
                }
                match tag.value.to_ascii_lowercase().as_str() {
                    "music" => Some(Self::Music),
                    "audiobook" => Some(Self::Audiobook),
                    _ => None,
                }
            }) {
                return kind;
            }
            let book = title.tags.iter().any(|tag| {
                !tag.value.trim().is_empty()
                    && (matches!(tag.key.to_ascii_lowercase().as_str(), "author" | "narrator")
                        || (matches!(
                            tag.key.to_ascii_lowercase().as_str(),
                            "genre" | "releasetype"
                        ) && tag
                            .value
                            .split([';', '/'])
                            .any(|value| value.trim().eq_ignore_ascii_case("audiobook"))))
            });
            return if book { Self::Audiobook } else { Self::Music };
        }
        if let Some(kind) = title.tags.iter().find_map(|tag| {
            if !tag.key.eq_ignore_ascii_case("media_type") {
                return None;
            }
            match tag.value.trim().to_ascii_lowercase().as_str() {
                "video" => Some(Self::Video),
                "extra" | "extras" | "bonus" => Some(Self::Extras),
                "film" | "movie" => Some(Self::Film),
                "tv" | "television" => Some(Self::Television),
                _ => None,
            }
        }) {
            return kind;
        }
        if let Some(kind) = title.video_metadata.as_ref().and_then(Self::from_video) {
            return kind;
        }
        if title.tags.iter().any(|tag| {
            matches!(
                tag.key.to_ascii_lowercase().as_str(),
                "show" | "season_number" | "episode_id"
            ) && !tag.value.trim().is_empty()
        }) {
            return Self::Television;
        }
        // The title's confirmed identity takes precedence over pending proposals
        // and source-wide episode context, including mixed-content discs.
        title
            .video_metadata
            .as_ref()
            .and_then(Self::from_video)
            .or_else(|| {
                catalog
                    .video_metadata_suggestions
                    .iter()
                    .find(|suggestion| suggestion.title_id == title.id)
                    .and_then(|suggestion| Self::from_video(&suggestion.metadata))
            })
            .unwrap_or_else(|| {
                if catalog.video_episode_context.is_some() {
                    Self::Television
                } else {
                    inferred_video_kind(&catalog.titles, title).unwrap_or(Self::Video)
                }
            })
    }

    pub fn from_video(metadata: &VideoMetadata) -> Option<Self> {
        match metadata
            .content_type
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mainmovie" | "movie" | "film") => Some(Self::Film),
            Some("episode" | "probableepisode" | "series" | "tv") => Some(Self::Television),
            Some("video") => Some(Self::Video),
            Some("extra" | "extras" | "bonus") => Some(Self::Extras),
            _ if metadata.season.is_some() || metadata.episode.is_some() => Some(Self::Television),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "Video metadata",
            Self::Extras => "Extras metadata",
            Self::Film => "Film metadata",
            Self::Television => "Television metadata",
            Self::Music => "Music metadata",
            Self::Audiobook => "Audiobook metadata",
        }
    }

    pub fn fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Video | Self::Extras | Self::Film => &[
                ("title", "Title"),
                ("date", "Release date"),
                ("genre", "Genre"),
                ("comment", "Description"),
            ],
            Self::Television => &[
                ("title", "Episode title"),
                ("show", "Series"),
                ("season_number", "Season"),
                ("episode_id", "Episode"),
                ("genre", "Genre"),
                ("date", "Release date"),
                ("comment", "Description"),
            ],
            Self::Music => &[
                ("title", "Track title"),
                ("album", "Album"),
                ("artist", "Artist"),
                ("album_artist", "Album artist"),
                ("track", "Track number"),
                ("discnumber", "Disc number"),
                ("date", "Release date"),
                ("genre", "Genre"),
            ],
            Self::Audiobook => &[
                ("title", "Chapter title"),
                ("album", "Book"),
                ("author", "Author"),
                ("narrator", "Narrator"),
                ("series", "Book series"),
                ("series-part", "Book number"),
                ("comment", "Description"),
                ("track", "Chapter number"),
                ("discnumber", "Disc number"),
                ("date", "Release date"),
                ("genre", "Genre"),
            ],
        }
    }
}
