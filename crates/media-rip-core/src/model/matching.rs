//! Text comparison is supporting evidence, never proof of release identity.
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Comparison key tolerating Unicode composition, accents, case and punctuation.
/// Edition, volume and abridgement words and numbers are deliberately retained.
pub fn text_key(value: &str) -> String {
    value
        .replace('&', "and")
        .nfd()
        .filter(|c| !is_combining_mark(*c) && c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_keys_tolerate_presentation_but_retain_identity_qualifiers() {
        for (first, second) in [
            ("Beyoncé", "BEYONCE\u{301}"),
            ("Simon & Garfunkel", "Simon and Garfunkel"),
            ("Wall-E", "WALL E"),
        ] {
            assert_eq!(text_key(first), text_key(second));
        }
        for (first, second) in [
            ("Volume 1", "Volume 2"),
            ("Book (abridged)", "Book (unabridged)"),
            ("Film", "Film: The Return"),
            ("Album", "Album Deluxe Edition"),
        ] {
            assert_ne!(text_key(first), text_key(second));
        }
    }
}
