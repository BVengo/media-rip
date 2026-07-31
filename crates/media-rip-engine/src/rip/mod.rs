mod ffmpeg;

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    sync::mpsc::Sender,
};

use media_rip_core::{
    EngineEvent, LogLevel, MediaEngineError, RipRequest, ports::CancellationToken,
    util::ensure_mkv_extension,
};

pub(crate) fn run(
    request: RipRequest,
    events: &Sender<EngineEvent>,
    cancellation: &CancellationToken,
) -> Result<(), MediaEngineError> {
    if request.selections.is_empty() {
        return Err(MediaEngineError::Other(
            "select at least one title before saving".to_owned(),
        ));
    }
    if !crate::probe::tool_availability().ffmpeg {
        return Err(MediaEngineError::MissingTool(
            "ffmpeg (from the open-source FFmpeg project)".to_owned(),
        ));
    }

    fs::create_dir_all(&request.output_directory).map_err(|error| {
        MediaEngineError::Other(format!(
            "could not create {}: {error}",
            request.output_directory.display()
        ))
    })?;
    validate_output_directory(&request.output_directory)?;
    preflight(&request)?;

    let _ = events.send(EngineEvent::RipStarted {
        title_count: request.selections.len(),
    });
    let _ = events.send(EngineEvent::Log {
        level: LogLevel::Info,
        message: format!(
            "Saving {} selected title(s) to {}.",
            request.selections.len(),
            request.output_directory.display()
        ),
    });

    for (selection_index, selection) in request.selections.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Err(MediaEngineError::Cancelled);
        }
        let title = request
            .catalog
            .titles
            .iter()
            .find(|title| title.id == selection.title_id)
            .ok_or_else(|| {
                MediaEngineError::Other(format!(
                    "selected title {} is no longer available",
                    selection.title_id
                ))
            })?;

        ffmpeg::remux_title(
            &request,
            title,
            selection,
            selection_index,
            events,
            cancellation,
        )?;
    }

    let _ = events.send(EngineEvent::RipFinished {
        saved_titles: request.selections.len(),
        destination: request.output_directory,
    });
    Ok(())
}

fn preflight(request: &RipRequest) -> Result<(), MediaEngineError> {
    let mut destinations = HashSet::new();
    for selection in &request.selections {
        let title = request
            .catalog
            .titles
            .iter()
            .find(|title| title.id == selection.title_id)
            .ok_or_else(|| {
                MediaEngineError::Other(format!(
                    "selected title {} is no longer available",
                    selection.title_id
                ))
            })?;
        if selection.stream_indices.is_empty() {
            return Err(MediaEngineError::Other(format!(
                "{} has no selected tracks",
                title.name
            )));
        }
        if let Some(stream) = selection
            .stream_indices
            .iter()
            .find(|stream| !title.tracks.iter().any(|track| track.index == **stream))
        {
            return Err(MediaEngineError::Other(format!(
                "track {stream} is no longer available in {}",
                title.name
            )));
        }

        let destination = request
            .output_directory
            .join(ensure_mkv_extension(&selection.output_file));
        if !destinations.insert(destination.clone()) {
            return Err(MediaEngineError::Other(format!(
                "more than one selected title would be saved as {}",
                destination.display()
            )));
        }
        if destination.exists() && !request.overwrite {
            return Err(MediaEngineError::Other(format!(
                "{} already exists; choose a different output name",
                destination.display()
            )));
        }
    }
    Ok(())
}

fn validate_output_directory(directory: &std::path::Path) -> Result<(), MediaEngineError> {
    if !directory.is_dir() {
        return Err(MediaEngineError::Other(format!(
            "{} is not a directory",
            directory.display()
        )));
    }
    for suffix in 0..100 {
        let probe = directory.join(format!(
            ".mediarip-write-test-{}-{suffix}",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&probe) {
            Ok(_) => {
                let _ = fs::remove_file(probe);
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(MediaEngineError::Other(format!(
                    "cannot write to {}: {error}",
                    directory.display()
                )));
            }
        }
    }
    Err(MediaEngineError::Other(format!(
        "could not create a write test in {}",
        directory.display()
    )))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use media_rip_core::{
        DiscKind, MediaCatalog, MediaSource, RipSelection, StreamKind, Title, Track,
    };

    use super::*;

    #[test]
    fn preflight_rejects_duplicate_destinations_before_ripping() {
        let mut request = request();
        request.selections[1].output_file = "same".to_owned();
        request.selections[0].output_file = "same.mkv".to_owned();

        let error = preflight(&request).unwrap_err().to_string();
        assert!(error.contains("more than one selected title"));
    }

    #[test]
    fn preflight_rejects_stale_stream_indices() {
        let mut request = request();
        request.selections[0].stream_indices = vec![99];

        let error = preflight(&request).unwrap_err().to_string();
        assert!(error.contains("track 99 is no longer available"));
    }

    fn request() -> RipRequest {
        let titles = (0..2)
            .map(|id| Title {
                id,
                source_title_id: id.to_string(),
                name: format!("Title {id}"),
                is_main_title: id == 0,
                duration_ms: 1_000,
                size_bytes: None,
                size_is_estimate: false,
                chapters: Vec::new(),
                tracks: vec![Track {
                    index: 0,
                    kind: StreamKind::Video,
                    codec: "h264".to_owned(),
                    codec_long_name: None,
                    language: None,
                    name: None,
                    properties: Vec::new(),
                    selected: true,
                }],
                segment_count: None,
                segment_map: None,
                duplicate_key: None,
                output_file: format!("title-{id}.mkv"),
                selected: true,
            })
            .collect::<Vec<_>>();
        RipRequest {
            catalog: MediaCatalog {
                source: MediaSource {
                    id: "source".to_owned(),
                    name: "Source".to_owned(),
                    path: PathBuf::from("source.mkv"),
                    kind: DiscKind::File,
                    drive: None,
                    capacity_bytes: None,
                    encrypted: None,
                },
                titles,
                source_info: Vec::new(),
            },
            selections: vec![
                RipSelection {
                    title_id: 0,
                    stream_indices: vec![0],
                    output_file: "title-0.mkv".to_owned(),
                },
                RipSelection {
                    title_id: 1,
                    stream_indices: vec![0],
                    output_file: "title-1.mkv".to_owned(),
                },
            ],
            output_directory: PathBuf::from("unused"),
            overwrite: false,
        }
    }
}
