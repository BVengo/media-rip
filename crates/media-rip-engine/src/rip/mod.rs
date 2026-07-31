mod ffmpeg;

use std::sync::mpsc::Sender;

use media_rip_core::{
    EngineEvent, LogLevel, MediaEngineError, RipRequest, ports::CancellationToken,
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

    std::fs::create_dir_all(&request.output_directory).map_err(|error| {
        MediaEngineError::Other(format!(
            "could not create {}: {error}",
            request.output_directory.display()
        ))
    })?;

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
