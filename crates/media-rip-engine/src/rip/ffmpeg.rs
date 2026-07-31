use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};

use media_rip_core::{
    DiscKind, EngineEvent, LogLevel, MediaEngineError, OutputProfile, RipProgress, RipRequest,
    RipSelection, Title, ports::CancellationToken, util::ensure_extension,
};

use crate::process::ProcessSupervisor;

pub(super) fn validate_profile_available(profile: OutputProfile) -> Result<(), MediaEngineError> {
    let encoders = required_encoders(profile);
    if encoders.is_empty() {
        return Ok(());
    }
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map_err(|error| {
            MediaEngineError::Process(format!("could not inspect FFmpeg encoders: {error}"))
        })?;
    if !output.status.success() {
        return Err(MediaEngineError::Process(
            "FFmpeg could not report its available encoders".to_owned(),
        ));
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    for encoder in encoders {
        let available = listing.lines().any(|line| {
            line.split_whitespace()
                .nth(1)
                .is_some_and(|name| name == *encoder)
        });
        if !available {
            return Err(MediaEngineError::MissingTool(format!(
                "FFmpeg encoder {encoder}, required by {}",
                profile.display_name()
            )));
        }
    }
    Ok(())
}

fn required_encoders(profile: OutputProfile) -> &'static [&'static str] {
    match profile {
        OutputProfile::PreserveMkv => &[],
        OutputProfile::H264Mp4 => &["libx264", "aac"],
        OutputProfile::H265Mkv => &["libx265"],
        OutputProfile::Av1Webm => &["libaom-av1", "libopus"],
        OutputProfile::AudioFlac => &["flac"],
        OutputProfile::AudioOpus => &["libopus"],
        OutputProfile::AudioAac => &["aac"],
    }
}

pub(super) fn process_title(
    request: &RipRequest,
    title: &Title,
    selection: &RipSelection,
    selection_index: usize,
    events: &Sender<EngineEvent>,
    cancellation: &CancellationToken,
) -> Result<(), MediaEngineError> {
    let output_name = ensure_extension(&selection.output_file, request.output_profile.extension());
    let destination = request.output_directory.join(output_name);
    if destination.exists() && !request.overwrite {
        return Err(MediaEngineError::Other(format!(
            "{} already exists; choose a different output name",
            destination.display()
        )));
    }

    let temporary = temporary_output_path(&destination, title.id);
    let extracted_cd = if request.catalog.source.kind == DiscKind::AudioCd {
        Some(TemporaryCdAudio(extract_audio_cd_track(
            request,
            title,
            &destination,
            events,
            cancellation,
        )?))
    } else {
        None
    };
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-nostdin", "-loglevel", "warning"]);
    add_input(
        &mut command,
        request,
        title,
        extracted_cd.as_ref().map(|audio| audio.0.as_path()),
    );
    for stream in &selection.stream_indices {
        command.args(["-map", &format!("0:{stream}")]);
    }
    command.args(["-map_chapters", "0"]);
    add_audio_cd_metadata(&mut command, request, title);
    add_output_profile(&mut command, request.output_profile);
    command.args(["-progress", "pipe:1", "-nostats"]);
    if request.overwrite {
        command.arg("-y");
    } else {
        command.arg("-n");
    }
    command.arg(&temporary);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let _ = events.send(EngineEvent::Log {
        level: LogLevel::Info,
        message: format!(
            "Processing title {} with {} as {}.",
            title.source_title_id,
            request.output_profile.display_name(),
            destination.display()
        ),
    });

    let mut process = ProcessSupervisor::spawn(&mut command)
        .map_err(|error| MediaEngineError::Process(format!("could not start ffmpeg: {error}")))?;
    let stdout = process.take_stdout().ok_or_else(|| {
        MediaEngineError::Process("ffmpeg progress pipe is unavailable".to_owned())
    })?;
    let stderr = process
        .take_stderr()
        .ok_or_else(|| MediaEngineError::Process("ffmpeg error pipe is unavailable".to_owned()))?;
    let (progress_tx, progress_rx) = mpsc::channel();
    let stderr_reader = thread::spawn(move || read_all(stderr));
    let progress_reader = thread::spawn(move || read_progress(stdout, progress_tx));
    let started = Instant::now();
    let mut last_emit = Instant::now() - Duration::from_secs(1);

    loop {
        if cancellation.is_cancelled() {
            let _ = process.terminate();
            cleanup_partial(&temporary);
            return Err(MediaEngineError::Cancelled);
        }

        while let Ok(progress) = progress_rx.try_recv() {
            if last_emit.elapsed() >= Duration::from_millis(150) || progress.finished {
                let fraction = if title.duration_ms > 0 {
                    (progress.out_time_ms as f64 / title.duration_ms as f64).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let elapsed_seconds = started.elapsed().as_secs();
                let remaining_seconds =
                    estimate_remaining(elapsed_seconds, fraction).filter(|_| !progress.finished);
                let read_rate_bytes = (elapsed_seconds > 0)
                    .then_some(progress.bytes_written / elapsed_seconds.max(1));
                let _ = events.send(EngineEvent::RipProgress(RipProgress {
                    current_title: selection_index + 1,
                    title_count: request.selections.len(),
                    title_name: title.name.clone(),
                    fraction,
                    bytes_written: Some(progress.bytes_written),
                    read_rate_bytes,
                    elapsed_seconds,
                    remaining_seconds,
                    destination: destination.clone(),
                }));
                last_emit = Instant::now();
            }
        }

        if let Some(status) = process
            .try_wait()
            .map_err(|error| MediaEngineError::Process(format!("ffmpeg wait failed: {error}")))?
        {
            let _ = progress_reader.join();
            let stderr = stderr_reader.join().unwrap_or_default();
            if !status.success() {
                cleanup_partial(&temporary);
                let details = stderr.trim();
                return Err(MediaEngineError::Process(if details.is_empty() {
                    format!("ffmpeg exited with {status}")
                } else {
                    details.to_owned()
                }));
            }

            if destination.exists() && request.overwrite {
                fs::remove_file(&destination).map_err(|error| {
                    MediaEngineError::Other(format!(
                        "could not replace {}: {error}",
                        destination.display()
                    ))
                })?;
            }
            fs::rename(&temporary, &destination).map_err(|error| {
                MediaEngineError::Other(format!(
                    "could not finalize {}: {error}",
                    destination.display()
                ))
            })?;
            let _ = events.send(EngineEvent::TitleFinished {
                title_name: title.name.clone(),
                destination,
            });
            return Ok(());
        }

        thread::sleep(Duration::from_millis(100));
    }
}

fn add_output_profile(command: &mut Command, profile: OutputProfile) {
    match profile {
        OutputProfile::PreserveMkv => {
            command.args(["-c", "copy", "-f", "matroska"]);
        }
        OutputProfile::H264Mp4 => {
            command.args([
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "20",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-c:s",
                "mov_text",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ]);
        }
        OutputProfile::H265Mkv => {
            command.args([
                "-c:v", "libx265", "-preset", "medium", "-crf", "22", "-c:a", "copy", "-c:s",
                "copy", "-c:t", "copy", "-f", "matroska",
            ]);
        }
        OutputProfile::Av1Webm => {
            command.args([
                "-c:v",
                "libaom-av1",
                "-crf",
                "30",
                "-b:v",
                "0",
                "-cpu-used",
                "4",
                "-c:a",
                "libopus",
                "-b:a",
                "128k",
                "-c:s",
                "webvtt",
                "-f",
                "webm",
            ]);
        }
        OutputProfile::AudioFlac => {
            command.args([
                "-vn",
                "-c:a",
                "flac",
                "-compression_level",
                "8",
                "-f",
                "flac",
            ]);
        }
        OutputProfile::AudioOpus => {
            command.args([
                "-vn", "-c:a", "libopus", "-b:a", "128k", "-vbr", "on", "-f", "opus",
            ]);
        }
        OutputProfile::AudioAac => {
            command.args([
                "-vn",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ]);
        }
    }
}

fn add_input(
    command: &mut Command,
    request: &RipRequest,
    title: &Title,
    extracted_cd: Option<&Path>,
) {
    let source = &request.catalog.source;
    match source.kind {
        DiscKind::Dvd => {
            command.args([
                "-f",
                "dvdvideo",
                "-title",
                &title.source_title_id,
                "-i",
                &source.path.to_string_lossy(),
            ]);
        }
        DiscKind::BluRay => {
            if title.source_title_id != "-1" {
                command.args(["-playlist", &title.source_title_id]);
            }
            command.arg("-i");
            command.arg(format!("bluray:{}", source.path.display()));
        }
        DiscKind::AudioCd => {
            command
                .arg("-i")
                .arg(extracted_cd.expect("Audio CD track must be extracted before encoding"));
        }
        DiscKind::Directory | DiscKind::File | DiscKind::Unknown => {
            command.arg("-i").arg(&source.path);
        }
    }
}

fn extract_audio_cd_track(
    request: &RipRequest,
    title: &Title,
    destination: &Path,
    events: &Sender<EngineEvent>,
    cancellation: &CancellationToken,
) -> Result<PathBuf, MediaEngineError> {
    let wave = cd_wave_path(destination, title.id);
    cleanup_partial(&wave);
    let _ = events.send(EngineEvent::Log {
        level: LogLevel::Info,
        message: format!(
            "Securely extracting Audio CD track {} with cdparanoia.",
            title.source_title_id
        ),
    });
    let mut command = Command::new("cdparanoia");
    command
        .args(["-q", "-d"])
        .arg(&request.catalog.source.path)
        .arg(&title.source_title_id)
        .arg(&wave)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut process = ProcessSupervisor::spawn(&mut command).map_err(|error| {
        MediaEngineError::MissingTool(format!(
            "cdparanoia (could not start secure Audio CD extraction: {error})"
        ))
    })?;
    let stderr = process.take_stderr().ok_or_else(|| {
        MediaEngineError::Process("cdparanoia error pipe is unavailable".to_owned())
    })?;
    let stderr_reader = thread::spawn(move || read_all(stderr));
    loop {
        if cancellation.is_cancelled() {
            let _ = process.terminate();
            let _ = stderr_reader.join();
            cleanup_partial(&wave);
            return Err(MediaEngineError::Cancelled);
        }
        if let Some(status) = process.try_wait().map_err(|error| {
            MediaEngineError::Process(format!("cdparanoia wait failed: {error}"))
        })? {
            let detail = stderr_reader.join().unwrap_or_default();
            if !status.success() {
                cleanup_partial(&wave);
                return Err(MediaEngineError::Process(if detail.trim().is_empty() {
                    format!("cdparanoia exited with {status}")
                } else {
                    detail.trim().to_owned()
                }));
            }
            if !wave.is_file() {
                return Err(MediaEngineError::Process(
                    "cdparanoia completed without producing track audio".to_owned(),
                ));
            }
            return Ok(wave);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn add_audio_cd_metadata(command: &mut Command, request: &RipRequest, title: &Title) {
    if request.catalog.source.kind != DiscKind::AudioCd {
        return;
    }
    for (key, value) in audio_cd_metadata(request, title) {
        command.args(["-metadata", &format!("{key}={value}")]);
    }
}

fn audio_cd_metadata(request: &RipRequest, title: &Title) -> Vec<(&'static str, String)> {
    let album_artist = request
        .catalog
        .source_info
        .iter()
        .find(|field| field.label == "Album artist")
        .map(|field| field.value.as_str());
    let artist = title
        .tracks
        .first()
        .and_then(|track| track.name.as_deref())
        .or(album_artist);
    let track_total = request
        .catalog
        .titles
        .iter()
        .filter_map(|title| title.source_title_id.parse::<u32>().ok())
        .max()
        .unwrap_or(request.catalog.titles.len() as u32);
    let track_number = if track_total > 0 {
        format!("{}/{track_total}", title.source_title_id)
    } else {
        title.source_title_id.clone()
    };

    [
        ("title", Some(title.name.as_str())),
        ("album", Some(request.catalog.source.name.as_str())),
        ("artist", artist),
        ("album_artist", album_artist),
        ("track", Some(track_number.as_str())),
    ]
    .into_iter()
    .filter_map(|(key, value)| {
        value
            .filter(|value| !value.is_empty())
            .map(|value| (key, value.to_owned()))
    })
    .collect()
}

fn cd_wave_path(destination: &Path, title_id: u32) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("track");
    destination.with_file_name(format!(
        ".{file_name}.{}.{}.cd.wav",
        std::process::id(),
        title_id
    ))
}

struct TemporaryCdAudio(PathBuf);

impl Drop for TemporaryCdAudio {
    fn drop(&mut self) {
        cleanup_partial(&self.0);
    }
}

#[derive(Debug, Default)]
struct ProgressState {
    out_time_ms: u64,
    bytes_written: u64,
    finished: bool,
}

fn read_progress(reader: impl Read, sender: Sender<ProgressState>) {
    let mut state = ProgressState::default();
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                // FFmpeg names this field `out_time_us` but its historical alias
                // `out_time_ms` is also expressed in microseconds.
                "out_time_us" | "out_time_ms" => {
                    state.out_time_ms = value.parse::<u64>().unwrap_or(0) / 1000;
                }
                "total_size" => state.bytes_written = value.parse().unwrap_or(0),
                "progress" => {
                    state.finished = value == "end";
                    let _ = sender.send(ProgressState {
                        out_time_ms: state.out_time_ms,
                        bytes_written: state.bytes_written,
                        finished: state.finished,
                    });
                }
                _ => {}
            }
        }
    }
}

fn read_all(mut reader: impl Read) -> String {
    let mut buffer = String::new();
    let _ = reader.read_to_string(&mut buffer);
    buffer
}

fn temporary_output_path(destination: &Path, title_id: u32) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("title.mkv");
    destination.with_file_name(format!(
        ".{file_name}.{}.{}.part",
        std::process::id(),
        title_id
    ))
}

fn cleanup_partial(path: &Path) {
    if path.is_file() {
        let _ = fs::remove_file(path);
    }
}

fn estimate_remaining(elapsed_seconds: u64, fraction: f64) -> Option<u64> {
    if !(0.0..=1.0).contains(&fraction) || fraction <= 0.001 {
        return None;
    }
    Some(((elapsed_seconds as f64 / fraction) - elapsed_seconds as f64).max(0.0) as u64)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use media_rip_core::{InfoField, MediaCatalog, MediaSource, StreamKind, Track};

    use super::*;

    #[test]
    fn estimates_remaining_from_progress() {
        assert_eq!(estimate_remaining(30, 0.25), Some(90));
        assert_eq!(estimate_remaining(30, 0.0), None);
    }

    #[test]
    fn produces_hidden_partial_beside_destination() {
        let path = temporary_output_path(Path::new("/video/Movie.mkv"), 3);
        assert_eq!(path.parent(), Some(Path::new("/video")));
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".Movie.mkv.")
        );
    }

    #[test]
    fn profile_arguments_select_the_requested_container_and_codecs() {
        let mut command = Command::new("ffmpeg");
        add_output_profile(&mut command, OutputProfile::H264Mp4);
        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(arguments.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
        assert!(arguments.windows(2).any(|pair| pair == ["-f", "mp4"]));
    }

    #[test]
    fn audio_profiles_disable_video_and_select_the_requested_encoder() {
        for (profile, encoder, container) in [
            (OutputProfile::AudioFlac, "flac", "flac"),
            (OutputProfile::AudioOpus, "libopus", "opus"),
            (OutputProfile::AudioAac, "aac", "mp4"),
        ] {
            let mut command = Command::new("ffmpeg");
            add_output_profile(&mut command, profile);
            let arguments = command
                .get_args()
                .map(|value| value.to_string_lossy())
                .collect::<Vec<_>>();
            assert!(arguments.iter().any(|argument| argument == "-vn"));
            assert!(arguments.windows(2).any(|pair| pair == ["-c:a", encoder]));
            assert!(arguments.windows(2).any(|pair| pair == ["-f", container]));
        }
    }

    #[test]
    fn audio_cd_wave_is_hidden_beside_the_destination() {
        let destination = Path::new("/music/01 - Track.flac");
        let wave = cd_wave_path(destination, 7);
        assert_eq!(wave.parent(), destination.parent());
        let name = wave.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with(".01 - Track.flac."));
        assert!(name.ends_with(".7.cd.wav"));
    }

    #[test]
    fn audio_cd_metadata_includes_album_artist_and_track_total() {
        let titles = (1..=12)
            .map(|number| Title {
                id: number - 1,
                source_title_id: number.to_string(),
                name: format!("Chapter {number}"),
                is_main_title: false,
                duration_ms: 60_000,
                size_bytes: None,
                size_is_estimate: false,
                chapters: Vec::new(),
                tracks: vec![Track {
                    index: 0,
                    kind: StreamKind::Audio,
                    codec: "PCM".to_owned(),
                    codec_long_name: None,
                    language: None,
                    name: None,
                    properties: Vec::new(),
                    selected: true,
                }],
                segment_count: None,
                segment_map: None,
                duplicate_key: None,
                output_file: format!("{number:02}.flac"),
                selected: true,
            })
            .collect::<Vec<_>>();
        let request = RipRequest {
            catalog: MediaCatalog {
                source: MediaSource {
                    id: "cd".to_owned(),
                    name: "The Book".to_owned(),
                    path: PathBuf::from("/dev/cdrom"),
                    kind: DiscKind::AudioCd,
                    drive: None,
                    capacity_bytes: None,
                    encrypted: None,
                },
                source_info: vec![InfoField::new("Album artist", "The Narrator")],
                titles,
            },
            selections: Vec::new(),
            output_directory: PathBuf::new(),
            overwrite: false,
            output_profile: OutputProfile::AudioFlac,
        };

        let metadata = audio_cd_metadata(&request, &request.catalog.titles[8]);

        assert!(metadata.contains(&("title", "Chapter 9".to_owned())));
        assert!(metadata.contains(&("album", "The Book".to_owned())));
        assert!(metadata.contains(&("artist", "The Narrator".to_owned())));
        assert!(metadata.contains(&("album_artist", "The Narrator".to_owned())));
        assert!(metadata.contains(&("track", "9/12".to_owned())));
    }

    #[test]
    fn every_encoding_profile_declares_required_encoders() {
        assert!(required_encoders(OutputProfile::PreserveMkv).is_empty());
        for profile in OutputProfile::ALL
            .into_iter()
            .filter(|profile| *profile != OutputProfile::PreserveMkv)
        {
            assert!(!required_encoders(profile).is_empty());
        }
    }
}
