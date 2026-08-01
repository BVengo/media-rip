mod ffprobe;

use std::{
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::Sender,
};

use media_rip_core::{
    DiscKind, Drive, EngineEvent, InfoField, LogLevel, MediaCatalog, MediaEngineError, MediaSource,
    ScanOptions,
    ports::CancellationToken,
    util::{format_bytes, sanitize_file_name},
};

use crate::source;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolAvailability {
    pub ffmpeg: bool,
    pub ffprobe: bool,
    pub cdparanoia: bool,
}

pub fn tool_availability() -> ToolAvailability {
    ToolAvailability {
        ffmpeg: tool_exists("ffmpeg"),
        ffprobe: tool_exists("ffprobe"),
        cdparanoia: command_succeeds("cdparanoia", "-V"),
    }
}

pub(crate) fn scan_source(
    path: &Path,
    options: ScanOptions,
    events: &Sender<EngineEvent>,
    cancellation: &CancellationToken,
) -> Result<MediaCatalog, MediaEngineError> {
    if !path.exists() {
        return Err(MediaEngineError::MissingInput(path.to_owned()));
    }
    let drive = crate::platform::discover_optical_drives()
        .ok()
        .and_then(|drives| {
            drives.into_iter().find(|drive| {
                drive.device_path == path || drive.mount_path.as_deref() == Some(path)
            })
        });
    let detected_kind = source::detect_kind(path);
    let kind = if detected_kind == DiscKind::Unknown {
        drive
            .as_ref()
            .map(|drive| drive.media_kind)
            .unwrap_or(detected_kind)
    } else {
        detected_kind
    };
    if kind != DiscKind::AudioCd && !tool_availability().ffprobe {
        return Err(MediaEngineError::MissingTool(
            "ffprobe (from the open-source FFmpeg project)".to_owned(),
        ));
    }
    let media_path = match kind {
        DiscKind::Dvd => source::normalize_disc_root(path, "VIDEO_TS"),
        DiscKind::BluRay => source::normalize_disc_root(path, "BDMV"),
        _ => path.to_owned(),
    };
    log(
        events,
        LogLevel::Info,
        format!(
            "Opening {} source at {}.",
            kind.display_name(),
            path.display()
        ),
    );
    let (
        mut titles,
        disc_album,
        disc_artist,
        online_tagged,
        exact_musicbrainz_match,
        musicbrainz_error,
        cover_art,
    ) = if kind == DiscKind::AudioCd {
        let catalog = source::audio_cd_catalog(
            &media_path,
            options.auto_tag_audio && options.allow_online_access,
            cancellation,
        )?;
        let online_tagged = catalog.titles.iter().any(|title| {
            title
                .tags
                .iter()
                .any(|tag| tag.key == "musicbrainz_albumid")
        });
        (
            catalog.titles,
            catalog.album,
            catalog.artist,
            online_tagged,
            catalog.exact_musicbrainz_match,
            catalog.musicbrainz_error,
            catalog.cover_art,
        )
    } else {
        let candidates = source::candidates_for(&media_path, kind)?;
        log(
            events,
            LogLevel::Info,
            format!("Discovered {} candidate title(s).", candidates.len()),
        );
        let mut titles = Vec::new();
        let mut errors = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(MediaEngineError::Cancelled);
            }
            log(
                events,
                LogLevel::Info,
                format!(
                    "Reading title {} of {} ({})…",
                    index + 1,
                    candidates.len(),
                    candidate.source_title_id
                ),
            );
            let _ = events.send(EngineEvent::ScanProgress {
                current_title: index + 1,
                title_count: candidates.len(),
                source_title_id: candidate.source_title_id.clone(),
            });

            match ffprobe::probe_title(candidate, cancellation) {
                Ok(title) => titles.push(title),
                Err(MediaEngineError::Cancelled) => return Err(MediaEngineError::Cancelled),
                Err(error) => {
                    errors.push(error.to_string());
                    log(
                        events,
                        LogLevel::Warning,
                        format!("Skipped title {}: {error}", candidate.source_title_id),
                    );
                }
            }
        }
        if titles.is_empty() {
            let details = errors
                .first()
                .cloned()
                .unwrap_or_else(|| "no readable titles were found".to_owned());
            return Err(MediaEngineError::Process(details));
        }
        if !errors.is_empty() {
            log(
                events,
                LogLevel::Warning,
                format!(
                    "{} candidate title(s) were skipped because they could not be read.",
                    errors.len()
                ),
            );
        }
        (titles, None, None, false, false, None, None)
    };

    if kind == DiscKind::AudioCd {
        log(
            events,
            LogLevel::Info,
            format!("Discovered {} audio track(s).", titles.len()),
        );
        if options.auto_tag_audio && options.allow_online_access {
            log(
                events,
                if online_tagged {
                    LogLevel::Success
                } else {
                    LogLevel::Warning
                },
                if online_tagged {
                    if exact_musicbrainz_match {
                        "Applied tags from an exact MusicBrainz disc match.".to_owned()
                    } else {
                        "Applied MusicBrainz tags from a release matching the CD-TEXT track list."
                            .to_owned()
                    }
                } else if let Some(error) = &musicbrainz_error {
                    format!("MusicBrainz lookup failed ({error}); using CD-TEXT or generic tags.")
                } else {
                    "No exact MusicBrainz disc match was available; using CD-TEXT or generic tags."
                        .to_owned()
                },
            );
        }
    }

    for (index, title) in titles.iter_mut().enumerate() {
        title.id = index as u32;
        if kind != DiscKind::AudioCd {
            title.output_file = format!(
                "{}_t{index:02}.mkv",
                sanitize_file_name(&source::source_stem(path))
            );
        }
    }

    let capacity = drive
        .as_ref()
        .and_then(|drive| drive.capacity_bytes)
        .or_else(|| {
            media_path
                .is_file()
                .then(|| media_path.metadata().ok().map(|metadata| metadata.len()))
                .flatten()
        });
    let source_name = disc_album.clone().unwrap_or_else(|| {
        if kind == DiscKind::AudioCd {
            "Audio CD".to_owned()
        } else {
            source::source_stem(&media_path)
        }
    });
    let mut source_info =
        source_information(&media_path, kind, &source_name, capacity, drive.as_ref());
    if kind == DiscKind::AudioCd {
        source_info.insert(
            2,
            InfoField::new(
                "Tag source",
                if online_tagged {
                    if exact_musicbrainz_match {
                        "MusicBrainz (exact disc match)"
                    } else {
                        "MusicBrainz (CD-TEXT release match)"
                    }
                } else if disc_album.is_some() || disc_artist.is_some() {
                    "CD-TEXT"
                } else {
                    "Generic track information"
                },
            ),
        );
        if online_tagged {
            source_info.insert(
                3,
                InfoField::new(
                    "Cover artwork",
                    if cover_art.is_some() {
                        "Cover Art Archive (embedded in supported audio outputs)"
                    } else {
                        "No front cover was available for this release"
                    },
                ),
            );
        }
    }
    if let Some(artist) = disc_artist {
        source_info.insert(2, InfoField::new("Album artist", artist));
    }
    log(
        events,
        LogLevel::Success,
        format!(
            "Source opened successfully; {} title(s) available.",
            titles.len()
        ),
    );

    Ok(MediaCatalog {
        source: MediaSource {
            id: media_path.to_string_lossy().into_owned(),
            name: source_name,
            path: media_path,
            kind,
            drive,
            capacity_bytes: capacity,
            encrypted: None,
        },
        titles,
        source_info,
        cover_art,
        track_cover_art: Default::default(),
    })
}

fn source_information(
    path: &Path,
    kind: DiscKind,
    source_name: &str,
    capacity: Option<u64>,
    drive: Option<&Drive>,
) -> Vec<InfoField> {
    let mut fields = vec![
        InfoField::new("Type", kind.display_name()),
        InfoField::new("Name", source_name),
        InfoField::new("Source path", path.display().to_string()),
    ];
    if let Some(capacity) = capacity.filter(|capacity| *capacity > 0) {
        fields.push(InfoField::new("Size", format_bytes(capacity)));
    }
    if let Some(drive) = drive {
        fields.push(InfoField::new(
            "OS device name",
            drive.device_path.display().to_string(),
        ));
        if let Some(vendor) = &drive.vendor {
            fields.push(InfoField::new("Manufacturer", vendor));
        }
        if let Some(model) = &drive.model {
            fields.push(InfoField::new("Product", model));
        }
        if let Some(revision) = &drive.revision {
            fields.push(InfoField::new("Revision", revision));
        }
        if let Some(serial) = &drive.serial {
            fields.push(InfoField::new("Serial number", serial));
        }
        if let Some(bus) = &drive.bus {
            fields.push(InfoField::new("Bus", bus));
        }
    }
    fields.push(InfoField::new("Protection", "Not reported"));
    fields
}

fn log(events: &Sender<EngineEvent>, level: LogLevel, message: String) {
    let _ = events.send(EngineEvent::Log { level, message });
}

fn tool_exists(program: &str) -> bool {
    command_succeeds(program, "-version")
}

fn command_succeeds(program: &str, version_argument: &str) -> bool {
    Command::new(program)
        .arg(version_argument)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod live_tests {
    use std::{path::PathBuf, sync::mpsc};

    use super::*;

    #[test]
    #[ignore = "requires an Audio CD in a readable optical drive"]
    fn scans_a_live_audio_cd() {
        let device = std::env::var_os("MEDIA_RIP_CD_DEVICE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/dev/sr0"));
        let (events, _receiver) = mpsc::channel();
        let catalog = scan_source(
            &device,
            ScanOptions {
                auto_tag_audio: false,
                allow_online_access: false,
            },
            &events,
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(catalog.source.kind, DiscKind::AudioCd);
        assert!(!catalog.titles.is_empty());
        if let Ok(expected_album) = std::env::var("MEDIA_RIP_EXPECTED_CD_ALBUM") {
            assert_eq!(catalog.source.name, expected_album);
        }
        assert!(catalog.titles.iter().all(|title| {
            title.duration_ms > 0
                && title.size_bytes.is_some()
                && title.tracks.len() == 1
                && title.tracks[0].kind == media_rip_core::StreamKind::Audio
        }));
    }
}
