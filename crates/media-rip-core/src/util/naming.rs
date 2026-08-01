use std::path::{Path, PathBuf};

pub fn sanitize_file_name(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut previous_was_separator = false;

    for character in value.trim().chars() {
        let forbidden = matches!(
            character,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
        );
        let replacement = forbidden || character.is_control();

        if replacement {
            if !previous_was_separator {
                result.push('_');
            }
            previous_was_separator = true;
        } else {
            result.push(character);
            previous_was_separator = false;
        }
    }

    let result = result.trim_matches([' ', '.']).to_owned();
    if result.is_empty() {
        "title".to_owned()
    } else {
        result
    }
}

pub fn ensure_extension(value: &str, extension: &str) -> String {
    let clean = sanitize_relative_path(value);
    if Path::new(&clean)
        .extension()
        .is_some_and(|current| current.eq_ignore_ascii_case(extension))
    {
        clean
    } else {
        let path = Path::new(&clean);
        if path.extension().is_some_and(|current| {
            ["mkv", "mp4", "webm", "flac", "wav", "opus", "m4a"]
                .iter()
                .any(|known| current.eq_ignore_ascii_case(known))
        }) {
            let mut renamed = path.to_path_buf();
            renamed.set_extension(extension);
            renamed.to_string_lossy().into_owned()
        } else {
            format!("{clean}.{extension}")
        }
    }
}

/// Sanitizes every component of a user-provided relative output path while
/// preserving directory separators. Absolute paths and parent traversal are
/// deliberately reduced to safe, relative components.
pub fn sanitize_relative_path(value: &str) -> String {
    let mut path = PathBuf::new();
    for component in value.trim().split(['/', '\\']) {
        if component.is_empty() || component == "." {
            continue;
        }
        path.push(sanitize_file_name(component));
    }

    if path.as_os_str().is_empty() {
        "title".to_owned()
    } else {
        path.to_string_lossy().into_owned()
    }
}

pub fn ensure_mkv_extension(value: &str) -> String {
    ensure_extension(value, "mkv")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_cross_platform_file_names() {
        assert_eq!(sanitize_file_name(" Film: Part/One? "), "Film_ Part_One_");
        assert_eq!(sanitize_file_name("..."), "title");
    }

    #[test]
    fn adds_mkv_extension_once() {
        assert_eq!(ensure_mkv_extension("Movie"), "Movie.mkv");
        assert_eq!(ensure_mkv_extension("Movie.MKV"), "Movie.MKV");
    }

    #[test]
    fn changes_known_output_extensions_for_a_profile() {
        assert_eq!(ensure_extension("Movie.mkv", "mp4"), "Movie.mp4");
        assert_eq!(ensure_extension("Track.wav", "flac"), "Track.flac");
        assert_eq!(ensure_extension("Album.final", "flac"), "Album.final.flac");
    }

    #[test]
    fn preserves_safe_folders_and_sanitizes_each_component() {
        assert_eq!(
            ensure_extension("Artist/Album/1-01 - Song.flac", "flac"),
            "Artist/Album/1-01 - Song.flac"
        );
        assert_eq!(
            ensure_extension("/Artist/../Album\\01: Song.wav", "flac"),
            "Artist/title/Album/01_ Song.flac"
        );
    }
}
