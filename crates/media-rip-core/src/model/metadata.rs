//! Media-specific editing context derived from observed streams and metadata.
use super::{DiscKind, MediaCatalog, StreamKind, Title, VideoMetadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataKind {
    Video,
    Film,
    Television,
    Music,
    Audiobook,
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
                    Self::Video
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
            _ if metadata.season.is_some() || metadata.episode.is_some() => Some(Self::Television),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "Video metadata",
            Self::Film => "Film metadata",
            Self::Television => "Television metadata",
            Self::Music => "Music metadata",
            Self::Audiobook => "Audiobook metadata",
        }
    }

    pub fn fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Video | Self::Film => &[
                ("title", "Title"),
                ("date", "Release date"),
                ("genre", "Genre"),
                ("comment", "Description"),
            ],
            Self::Television => &[
                ("title", "Episode title"),
                ("show", "Series"),
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
                ("track", "Chapter number"),
                ("discnumber", "Disc number"),
                ("date", "Release date"),
                ("genre", "Genre"),
            ],
        }
    }
}
