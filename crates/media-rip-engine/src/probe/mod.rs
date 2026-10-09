mod ffprobe;

use std::{
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::Sender,
};

use media_rip_core::{
    DiscKind, Drive, EngineEvent, InfoField, LogLevel, MediaCatalog, MediaEngineError, MediaSource,
    ScanOptions, ports::CancellationToken, util::format_bytes,
};

use crate::{diagnostics, metadata, source};

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
        return Err(MediaEngineError::MissingTool("ffprobe".to_owned()));
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
    let mut audio_metadata_candidates = Vec::new();
    let (
        mut titles,
        disc_album,
        disc_artist,
        online_tagged,
        exact_musicbrainz_match,
        musicbrainz_error,
        mut cover_art,
    ) = if kind == DiscKind::AudioCd {
        log(
            events,
            LogLevel::Info,
            "Reading Audio CD track information and CD-TEXT metadata.".to_owned(),
        );
        if options.auto_tag_audio && options.allow_online_access {
            log(
                events,
                LogLevel::Info,
                "Looking for matching album metadata in MusicBrainz…".to_owned(),
            );
        } else {
            log(
                events,
                LogLevel::Info,
                if options.auto_tag_audio {
                    "Online album metadata lookup skipped because online access is disabled."
                } else {
                    "Online album metadata lookup is disabled; using CD-TEXT when available."
                }
                .to_owned(),
            );
        }
        let catalog = source::audio_cd_catalog(
            &media_path,
            options.auto_tag_audio && options.allow_online_access,
            cancellation,
        )?;
        audio_metadata_candidates = catalog.candidates;
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
        if kind == DiscKind::Dvd {
            let mut unique = std::collections::HashSet::new();
            let duplicates = candidates
                .iter()
                .filter(|candidate| {
                    candidate
                        .duplicate_key
                        .as_ref()
                        .is_some_and(|key| !unique.insert(key))
                })
                .count();
            let review = candidates.iter().filter(|candidate| matches!(&candidate.input, source::ProbeInput::Dvd { info: Some(info), .. } if !info.warnings.is_empty())).count();
            log(
                events,
                if review > 0 {
                    LogLevel::Warning
                } else {
                    LogLevel::Info
                },
                format!(
                    "DVD cell analysis: {} title references, {duplicates} duplicate paths, {review} layouts needing review. Title sizes describe cell extents, not separate copies of the disc.",
                    candidates.len()
                ),
            );
        }
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

    let fallback_video_name = drive
        .as_ref()
        .and_then(|drive| drive.media_label.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| source::source_stem(&media_path));
    let mut video_enrichment = if matches!(kind, DiscKind::Dvd | DiscKind::BluRay) {
        log(
            events,
            LogLevel::Info,
            match (options.auto_tag_video, options.allow_online_access) {
                (true, true) => {
                    "Discovering video metadata from authored disc data and enabled online providers…"
                }
                (true, false) => {
                    "Online video metadata lookup skipped because online access is disabled; checking authored disc metadata…"
                }
                (false, _) => {
                    "Online video metadata lookup is disabled; checking authored disc metadata…"
                }
            }
            .to_owned(),
        );
        let enrichment = metadata::enrich_video(
            &media_path,
            kind,
            &fallback_video_name,
            &mut titles,
            &options,
            cancellation,
        );
        if cancellation.is_cancelled() {
            return Err(MediaEngineError::Cancelled);
        }
        for warning in &enrichment.warnings {
            log(events, LogLevel::Warning, warning.clone());
        }
        if options.auto_tag_video && options.allow_online_access && !options.thediscdb_enabled {
            log(
                events,
                LogLevel::Info,
                "TheDiscDB lookup disabled in settings.".to_owned(),
            );
        } else if let Some(status) = &enrichment.disc_match_status {
            log(events, LogLevel::Info, format!("{status}."));
        }
        let tvmaze_link = options
            .video_metadata_url
            .as_deref()
            .is_some_and(|url| crate::metadata::video_metadata_link_provider(url) == Ok("TVMaze"));
        if options.auto_tag_video
            && options.allow_online_access
            && !options.tvmaze_enabled
            && !tvmaze_link
        {
            log(
                events,
                LogLevel::Info,
                "TVMaze enrichment disabled in settings.".to_owned(),
            );
        } else if let Some(status) = &enrichment.tvmaze_match_status {
            log(events, LogLevel::Info, format!("{status}."));
        }
        if options.auto_tag_video
            && options.allow_online_access
            && options
                .tmdb_access_token
                .as_deref()
                .is_none_or(|token| token.trim().is_empty())
        {
            log(
                events,
                LogLevel::Info,
                "TMDB enrichment skipped because no access token is configured.".to_owned(),
            );
        }
        if options.auto_tag_video
            && options.allow_online_access
            && options
                .tvdb_api_key
                .as_deref()
                .is_none_or(|key| key.trim().is_empty())
        {
            log(
                events,
                LogLevel::Info,
                "TVDB enrichment skipped because no API key is configured.".to_owned(),
            );
        }
        if !enrichment.attributions.is_empty() {
            log(
                events,
                LogLevel::Success,
                format!(
                    "Applied video metadata from {}.",
                    enrichment
                        .attributions
                        .iter()
                        .map(|attribution| attribution.provider.as_str())
                        .collect::<Vec<_>>()
                        .join(" + ")
                ),
            );
        }
        if !enrichment.suggestions.is_empty() {
            log(
                events,
                LogLevel::Info,
                format!(
                    "Found {} probable episode metadata suggestion(s) for review; none were applied automatically.",
                    enrichment.suggestions.len()
                ),
            );
        } else if enrichment.attributions.is_empty() {
            log(
                events,
                LogLevel::Info,
                "Metadata discovery completed without a match; keeping the discovered title names."
                    .to_owned(),
            );
        }
        if cover_art.is_none() {
            cover_art = enrichment.cover_art.clone();
        }
        Some(enrichment)
    } else {
        None
    };

    for (index, title) in titles.iter_mut().enumerate() {
        let old_id = title.id;
        if let Some(enrichment) = video_enrichment.as_mut() {
            for suggestion in enrichment
                .suggestions
                .iter_mut()
                .filter(|suggestion| suggestion.title_id == old_id)
            {
                suggestion.title_id = index as u32;
            }
        }
        title.id = index as u32;
    }
    if kind != DiscKind::AudioCd {
        metadata::assign_output_names(&mut titles, &source::source_stem(path));
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
            video_enrichment
                .as_ref()
                .and_then(|enrichment| enrichment.source_name.clone())
                .unwrap_or(fallback_video_name)
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
    if let Some(enrichment) = &video_enrichment {
        if let Some(content_type) = &enrichment.content_type {
            source_info.insert(2, InfoField::new("Content", content_type));
        }
        if let Some(year) = enrichment.year {
            source_info.insert(
                3.min(source_info.len()),
                InfoField::new("Year", year.to_string()),
            );
        }
        if let Some(hash) = &enrichment.content_hash {
            source_info.push(InfoField::new("Disc content hash", hash));
        }
        if let Some(status) = &enrichment.disc_match_status {
            source_info.push(InfoField::new("Disc metadata match", status));
        }
        if let Some(identity) = &enrichment.disc_identity {
            source_info.push(InfoField::new("Disc identity", identity));
        }
        if let Some(url) = &enrichment.identity_url {
            source_info.push(InfoField::new("Metadata URL", url));
        }
        if !enrichment.suggestions.is_empty() {
            source_info.push(InfoField::new(
                "Probable episode matches",
                format!(
                    "{} suggestion(s), not applied — review them in the content list",
                    enrichment.suggestions.len()
                ),
            ));
        }
        if !enrichment.attributions.is_empty() {
            source_info.push(InfoField::new(
                "Metadata source",
                enrichment
                    .attributions
                    .iter()
                    .map(|attribution| {
                        format!(
                            "{} ({}%, {})",
                            attribution.provider,
                            attribution.confidence_percent,
                            attribution.match_method
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        if cover_art.is_some() {
            source_info.push(InfoField::new(
                "Artwork",
                "Embedded disc metadata or an online metadata provider",
            ));
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

    let mut catalog = MediaCatalog {
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
        audio_metadata_candidates,
        metadata_attributions: video_enrichment
            .as_ref()
            .map(|enrichment| enrichment.attributions.clone())
            .unwrap_or_default(),
        video_metadata_suggestions: video_enrichment
            .as_ref()
            .map(|enrichment| enrichment.suggestions.clone())
            .unwrap_or_default(),
        video_episode_context: video_enrichment.and_then(|enrichment| enrichment.episode_context),
    };
    media_rip_core::model::dvd::infer_main_title(&mut catalog);
    diagnostics::catalog(&catalog);
    Ok(catalog)
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
                auto_tag_video: false,
                allow_online_access: false,
                thediscdb_enabled: false,
                tvmaze_enabled: false,
                tmdb_access_token: None,
                tvdb_api_key: None,
                tvdb_subscriber_pin: None,
                video_metadata_url: None,
                previous_dvd_match: None,
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
