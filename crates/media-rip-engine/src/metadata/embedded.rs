use std::{collections::HashMap, fs, io::BufReader, path::Path};

use media_rip_core::CoverArt;
use quick_xml::{Reader, events::Event};

use crate::source::{locate_child_case_insensitive, normalize_disc_root};

const MAX_ARTWORK_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct EmbeddedMetadata {
    pub disc_name: Option<String>,
    pub title_names: HashMap<u32, String>,
    pub artwork: Option<CoverArt>,
}

pub(super) fn read_bluray(path: &Path) -> EmbeddedMetadata {
    let root = normalize_disc_root(path, "BDMV");
    let bdmv = locate_child_case_insensitive(&root, "BDMV").unwrap_or_else(|| root.join("BDMV"));
    let Some(meta) = locate_child_case_insensitive(&bdmv, "META") else {
        return EmbeddedMetadata::default();
    };
    let Some(dl) = locate_child_case_insensitive(&meta, "DL") else {
        return EmbeddedMetadata::default();
    };
    let mut xml_files = fs::read_dir(&dl)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("xml"))
        })
        .collect::<Vec<_>>();
    xml_files.sort_by_key(|path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        (
            !name.eq_ignore_ascii_case("bdmt_eng.xml"),
            name.to_ascii_lowercase(),
        )
    });
    let Some(xml_path) = xml_files.first() else {
        return EmbeddedMetadata::default();
    };
    let Ok(file) = fs::File::open(xml_path) else {
        return EmbeddedMetadata::default();
    };
    let mut metadata = parse_bluray_xml(BufReader::new(file));
    if let Some(reference) = metadata
        .artwork
        .take()
        .and_then(|artwork| String::from_utf8(artwork.data).ok())
    {
        metadata.artwork = read_artwork(&dl.join(reference));
    }
    metadata
}

fn parse_bluray_xml(input: impl std::io::BufRead) -> EmbeddedMetadata {
    let mut reader = Reader::from_reader(input);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut metadata = EmbeddedMetadata::default();
    let mut element = Vec::<u8>::new();
    let mut title_number = None;
    let mut thumbnail = None;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(start)) => {
                element = start.local_name().as_ref().to_vec();
                if element == b"titleName" {
                    title_number = start.attributes().flatten().find_map(|attribute| {
                        (attribute.key.local_name().as_ref() == b"titleNumber")
                            .then(|| std::str::from_utf8(&attribute.value).ok()?.parse().ok())
                            .flatten()
                    });
                }
            }
            Ok(Event::Empty(start)) if start.local_name().as_ref() == b"thumbnail" => {
                let reference = start.attributes().flatten().find_map(|attribute| {
                    (attribute.key.local_name().as_ref() == b"href")
                        .then(|| String::from_utf8(attribute.value.into_owned()).ok())
                        .flatten()
                });
                if let Some(reference) = reference
                    && thumbnail.as_ref().is_none_or(|current: &String| {
                        thumbnail_rank(&reference) > thumbnail_rank(current)
                    })
                {
                    thumbnail = Some(reference);
                }
            }
            Ok(Event::Text(text)) => {
                let value = text.decode().ok().map(|value| value.trim().to_owned());
                if let Some(value) = value.filter(|value| !value.is_empty()) {
                    match element.as_slice() {
                        b"name" if metadata.disc_name.is_none() => metadata.disc_name = Some(value),
                        b"titleName" => {
                            if let Some(number) = title_number {
                                metadata.title_names.insert(number, value);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(_)) => {
                element.clear();
                title_number = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buffer.clear();
    }
    if let Some(thumbnail) = thumbnail {
        // Temporarily carry the relative reference without adding another field.
        metadata.artwork = Some(CoverArt::new("application/x-path", thumbnail.into_bytes()));
    }
    metadata
}

fn thumbnail_rank(reference: &str) -> (bool, usize) {
    (
        !reference.to_ascii_lowercase().contains("_sml"),
        reference.len(),
    )
}

fn read_artwork(path: &Path) -> Option<CoverArt> {
    let size = fs::metadata(path).ok()?.len();
    if size == 0 || size > MAX_ARTWORK_BYTES {
        return None;
    }
    let mime_type = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => return None,
    };
    Some(CoverArt::new(mime_type, fs::read(path).ok()?))
}

#[cfg(test)]
mod tests {
    use super::parse_bluray_xml;

    #[test]
    fn parses_names_and_prefers_the_larger_thumbnail_reference() {
        let xml = br#"<?xml version="1.0"?>
        <disclib xmlns:di="urn:BDA:bdmv;discinfo"><di:discinfo>
          <di:title><di:name>Example Film</di:name></di:title>
          <di:description><di:tableOfContents>
            <di:titleName titleNumber="1">The Feature</di:titleName>
            <di:titleName titleNumber="2">Making Of</di:titleName>
          </di:tableOfContents>
          <di:thumbnail href="small.jpg"/><di:thumbnail href="example_large.jpg"/>
          </di:description>
        </di:discinfo></disclib>"#;

        let metadata = parse_bluray_xml(&xml[..]);

        assert_eq!(metadata.disc_name.as_deref(), Some("Example Film"));
        assert_eq!(
            metadata.title_names.get(&1).map(String::as_str),
            Some("The Feature")
        );
        assert_eq!(
            metadata.artwork.unwrap().data,
            b"example_large.jpg".to_vec()
        );
    }
}
