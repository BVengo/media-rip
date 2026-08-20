use std::{
    collections::{HashMap, HashSet},
    io::Read,
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use media_rip_core::{
    Chapter, MediaEngineError, StreamKind, Title, Track, ports::CancellationToken,
    util::sanitize_file_name,
};
use serde::Deserialize;

use crate::{
    diagnostics,
    process::ProcessSupervisor,
    source::{ProbeCandidate, ProbeInput},
};

#[derive(Debug, Default, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    chapters: Vec<ProbeChapter>,
    #[serde(default)]
    format: ProbeFormat,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeStream {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    codec_name: Option<String>,
    #[serde(default)]
    codec_long_name: Option<String>,
    #[serde(default)]
    codec_type: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    #[serde(default)]
    sample_rate: Option<String>,
    #[serde(default)]
    channels: Option<u32>,
    #[serde(default)]
    channel_layout: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    bit_rate: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
    #[serde(default)]
    disposition: HashMap<String, i32>,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeChapter {
    id: u32,
    #[serde(default)]
    start_time: String,
    #[serde(default)]
    end_time: String,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeFormat {
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    bit_rate: Option<String>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct PacketProbeOutput {
    #[serde(default)]
    packets: Vec<ProbePacket>,
}

#[derive(Debug, Default, Deserialize)]
struct ProbePacket {
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    pts_time: Option<String>,
    #[serde(default)]
    dts_time: Option<String>,
}

pub(crate) fn probe_title(
    candidate: &ProbeCandidate,
    cancellation: &CancellationToken,
) -> Result<Title, MediaEngineError> {
    diagnostics::text(
        "MediaRip",
        "probe candidate (all authored disc information used to invoke ffprobe)",
        &format!("{candidate:#?}"),
    );
    let mut command = Command::new("ffprobe");
    command.args([
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
        "-show_chapters",
    ]);
    command.args(candidate.input.ffmpeg_input_args());

    let output = run_cancellable(command, cancellation)?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(MediaEngineError::Process(if error.is_empty() {
            format!("ffprobe exited with {}", output.status)
        } else {
            error
        }));
    }

    diagnostics::tool_json("ffprobe", &candidate.source_title_id, &output.stdout);
    let probe: ProbeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|error| MediaEngineError::Process(format!("invalid ffprobe metadata: {error}")))?;
    let mut title = title_from_probe(candidate, probe)?;
    if title.size_bytes.is_none()
        && let Some(size) =
            estimate_size_from_packet_sample(candidate, title.duration_ms, cancellation)?
    {
        title.size_bytes = Some(size);
        title.size_is_estimate = true;
    }
    Ok(title)
}

fn estimate_size_from_packet_sample(
    candidate: &ProbeCandidate,
    duration_ms: u64,
    cancellation: &CancellationToken,
) -> Result<Option<u64>, MediaEngineError> {
    if duration_ms == 0 {
        return Ok(None);
    }
    let mut command = Command::new("ffprobe");
    command.args([
        "-v",
        "error",
        "-read_intervals",
        "%+30",
        "-show_packets",
        "-show_entries",
        "packet=size,pts_time,dts_time",
        "-print_format",
        "json",
    ]);
    command.args(candidate.input.ffmpeg_input_args());
    let output = run_cancellable(command, cancellation)?;
    if !output.status.success() {
        return Ok(None);
    }
    diagnostics::tool_json(
        "ffprobe packet sample",
        &candidate.source_title_id,
        &output.stdout,
    );
    let Some(sample) = serde_json::from_slice(&output.stdout).ok() else {
        return Ok(None);
    };
    Ok(size_from_packet_sample(&sample, duration_ms))
}

fn run_cancellable(
    mut command: Command,
    cancellation: &CancellationToken,
) -> Result<Output, MediaEngineError> {
    if cancellation.is_cancelled() {
        return Err(MediaEngineError::Cancelled);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut process = ProcessSupervisor::spawn(&mut command)
        .map_err(|error| MediaEngineError::Process(format!("could not run ffprobe: {error}")))?;
    let stdout = process.take_stdout().ok_or_else(|| {
        MediaEngineError::Process("ffprobe output pipe is unavailable".to_owned())
    })?;
    let stderr = process
        .take_stderr()
        .ok_or_else(|| MediaEngineError::Process("ffprobe error pipe is unavailable".to_owned()))?;
    let stdout_reader = thread::spawn(move || read_bytes(stdout));
    let stderr_reader = thread::spawn(move || read_bytes(stderr));

    loop {
        if cancellation.is_cancelled() {
            let _ = process.terminate();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(MediaEngineError::Cancelled);
        }
        if let Some(status) = process
            .try_wait()
            .map_err(|error| MediaEngineError::Process(format!("ffprobe wait failed: {error}")))?
        {
            return Ok(Output {
                status,
                stdout: stdout_reader.join().unwrap_or_default(),
                stderr: stderr_reader.join().unwrap_or_default(),
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn read_bytes(mut reader: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = reader.read_to_end(&mut bytes);
    bytes
}

fn size_from_packet_sample(sample: &PacketProbeOutput, duration_ms: u64) -> Option<u64> {
    let mut bytes = 0_u64;
    let mut first_timestamp = f64::INFINITY;
    let mut last_timestamp = f64::NEG_INFINITY;
    for packet in &sample.packets {
        if let Some(size) = packet.size.as_deref().and_then(parse_positive_u64) {
            bytes = bytes.checked_add(size)?;
        }
        if let Some(timestamp) = packet
            .pts_time
            .as_deref()
            .and_then(|timestamp| timestamp.parse::<f64>().ok())
            .or_else(|| {
                packet
                    .dts_time
                    .as_deref()
                    .and_then(|timestamp| timestamp.parse::<f64>().ok())
            })
            .filter(|timestamp| timestamp.is_finite())
        {
            first_timestamp = first_timestamp.min(timestamp);
            last_timestamp = last_timestamp.max(timestamp);
        }
    }
    let sampled_seconds = last_timestamp - first_timestamp;
    if bytes == 0 || !sampled_seconds.is_finite() || sampled_seconds < 1.0 {
        return None;
    }
    let bit_rate = ((bytes as f64 * 8.0) / sampled_seconds) as u64;
    estimate_size(duration_ms, bit_rate)
}

fn title_from_probe(
    candidate: &ProbeCandidate,
    mut probe: ProbeOutput,
) -> Result<Title, MediaEngineError> {
    if probe.streams.is_empty() {
        return Err(MediaEngineError::Process(
            "the title contains no streams".to_owned(),
        ));
    }

    if matches!(&candidate.input, ProbeInput::BluRay { .. }) {
        retain_authored_bluray_streams(&mut probe.streams, candidate);
        deduplicate_bluray_tracks(&mut probe.streams);
    }

    let has_video_bit_rate = probe.streams.iter().any(|stream| {
        stream.codec_type.as_deref() == Some("video")
            && stream
                .bit_rate
                .as_deref()
                .and_then(parse_positive_u64)
                .is_some()
    });
    let stream_bit_rate = has_video_bit_rate
        .then(|| {
            probe
                .streams
                .iter()
                .filter_map(|stream| stream.bit_rate.as_deref().and_then(parse_positive_u64))
                .try_fold(0_u64, u64::checked_add)
        })
        .flatten()
        .filter(|bit_rate| *bit_rate > 0);
    let tracks = probe
        .streams
        .into_iter()
        .map(|stream| track_from_probe(stream, &candidate.stream_languages))
        .collect();
    let ffprobe_chapters = probe
        .chapters
        .into_iter()
        .map(|chapter| Chapter {
            index: chapter.id,
            start_ms: parse_milliseconds(&chapter.start_time).unwrap_or(0),
            end_ms: parse_milliseconds(&chapter.end_time).unwrap_or(0),
            name: tag(&chapter.tags, "title"),
        })
        .collect::<Vec<_>>();
    let chapters = candidate
        .chapters
        .clone()
        .filter(|chapters| !chapters.is_empty())
        .unwrap_or(ffprobe_chapters);

    let duration_ms = probe
        .format
        .duration
        .as_deref()
        .and_then(parse_milliseconds)
        .or_else(|| chapters.last().map(|chapter| chapter.end_ms))
        .unwrap_or(0);
    let exact_size = probe.format.size.as_deref().and_then(parse_positive_u64);
    let bit_rate = probe
        .format
        .bit_rate
        .as_deref()
        .and_then(parse_positive_u64)
        .or(stream_bit_rate);
    let (size_bytes, size_is_estimate) = if let Some(size) = exact_size {
        (Some(size), false)
    } else if let Some(size) = candidate
        .size_hint
        .filter(|size| *size > 0 && !candidate.size_hint_is_estimate)
    {
        (Some(size), false)
    } else if let Some(size) = bit_rate.and_then(|bit_rate| estimate_size(duration_ms, bit_rate)) {
        (Some(size), true)
    } else {
        (
            candidate.size_hint.filter(|size| *size > 0),
            candidate.size_hint_is_estimate,
        )
    };
    let name = tag(&probe.format.tags, "title").unwrap_or_else(|| candidate.display_name.clone());

    Ok(Title {
        id: candidate.id,
        source_title_id: candidate.source_title_id.clone(),
        disc_title_number: candidate.disc_title_number,
        output_file: format!("{}.mkv", sanitize_file_name(&name)),
        name,
        is_main_title: candidate.is_main_title,
        duration_ms,
        size_bytes,
        size_is_estimate,
        chapters,
        tracks,
        segment_count: candidate.segment_count,
        segment_map: candidate.segment_map.clone(),
        duplicate_key: candidate.duplicate_key.clone(),
        tags: Vec::new(),
        video_metadata: None,
        selected: true,
    })
}

fn retain_authored_bluray_streams(streams: &mut Vec<ProbeStream>, candidate: &ProbeCandidate) {
    let Some(authored_pids) = candidate
        .authored_stream_pids
        .as_ref()
        .filter(|pids| !pids.is_empty())
    else {
        return;
    };

    streams.retain(|stream| {
        stream
            .id
            .as_deref()
            .and_then(parse_stream_pid)
            .is_none_or(|pid| authored_pids.contains(&pid))
    });
}

fn deduplicate_bluray_tracks(streams: &mut Vec<ProbeStream>) {
    let mut seen_pids = HashSet::new();
    streams.retain(|stream| {
        if !matches!(stream.codec_type.as_deref(), Some("audio" | "subtitle")) {
            return true;
        }

        stream
            .id
            .as_deref()
            .and_then(parse_stream_pid)
            .is_none_or(|pid| seen_pids.insert(pid))
    });
}

fn track_from_probe(stream: ProbeStream, stream_languages: &HashMap<u16, String>) -> Track {
    let kind = match stream.codec_type.as_deref() {
        Some("video") => StreamKind::Video,
        Some("audio") => StreamKind::Audio,
        Some("subtitle") => StreamKind::Subtitle,
        Some("attachment") => StreamKind::Attachment,
        _ => StreamKind::Data,
    };
    let codec = stream
        .codec_name
        .clone()
        .unwrap_or_else(|| "unknown".to_owned())
        .to_ascii_uppercase();
    let mut properties = Vec::new();

    if let (Some(width), Some(height)) = (stream.width, stream.height) {
        properties.push(format!("{width}x{height}"));
    }
    if let Some(profile) = stream.profile.as_deref().filter(|value| !value.is_empty()) {
        properties.push(profile.to_owned());
    }
    if let Some(rate) = stream.r_frame_rate.as_deref().and_then(format_frame_rate) {
        properties.push(rate);
    }
    if let Some(layout) = stream
        .channel_layout
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        properties.push(layout.replace('_', " "));
    } else if let Some(channels) = stream.channels {
        properties.push(format!("{channels} channels"));
    }
    if let Some(rate) = stream
        .sample_rate
        .as_deref()
        .and_then(|value| value.parse::<u32>().ok())
    {
        properties.push(format!("{:.1} kHz", f64::from(rate) / 1000.0));
    }
    if stream.disposition.get("forced").copied().unwrap_or(0) == 1 {
        properties.push("forced only".to_owned());
    }
    if stream.disposition.get("default").copied().unwrap_or(0) == 1 {
        properties.push("default".to_owned());
    }
    if let Some(milliseconds) = stream.duration.as_deref().and_then(parse_milliseconds)
        && milliseconds > 0
    {
        properties.push(media_rip_core::util::format_duration(milliseconds));
    }

    Track {
        index: stream.index,
        kind,
        codec,
        codec_long_name: stream.codec_long_name,
        language: tag(&stream.tags, "language").or_else(|| {
            stream
                .id
                .as_deref()
                .and_then(parse_stream_pid)
                .and_then(|pid| stream_languages.get(&pid))
                .cloned()
        }),
        name: tag(&stream.tags, "title"),
        properties,
        selected: !matches!(kind, StreamKind::Data),
    }
}

fn parse_stream_pid(value: &str) -> Option<u16> {
    let value = value.trim();
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || value.parse().ok(),
            |hex| u16::from_str_radix(hex, 16).ok(),
        )
}

fn parse_milliseconds(value: &str) -> Option<u64> {
    let seconds = value.parse::<f64>().ok()?;
    seconds
        .is_finite()
        .then_some((seconds.max(0.0) * 1000.0) as u64)
}

fn parse_positive_u64(value: &str) -> Option<u64> {
    value.parse::<u64>().ok().filter(|parsed| *parsed > 0)
}

fn estimate_size(duration_ms: u64, bit_rate: u64) -> Option<u64> {
    let bytes = u128::from(duration_ms)
        .checked_mul(u128::from(bit_rate))?
        .checked_div(8_000)?;
    u64::try_from(bytes).ok().filter(|bytes| *bytes > 0)
}

fn tag(tags: &HashMap<String, String>, wanted: &str) -> Option<String> {
    tags.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn format_frame_rate(value: &str) -> Option<String> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator > 0.0).then(|| format!("{:.3} fps", numerator / denominator))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::source::ProbeInput;

    use super::*;

    #[test]
    fn cancelled_probe_does_not_start_a_process() {
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let error = run_cancellable(Command::new("program-that-must-not-start"), &cancellation)
            .unwrap_err();
        assert!(matches!(error, MediaEngineError::Cancelled));
    }

    #[test]
    fn parses_ffprobe_title_and_track_metadata() {
        let json = include_str!("../../../../tests/fixtures/ffprobe-title.json");
        let probe: ProbeOutput = serde_json::from_str(json).unwrap();
        let title = title_from_probe(
            &ProbeCandidate {
                id: 0,
                source_title_id: "00800".to_owned(),
                disc_title_number: None,
                display_name: "Feature".to_owned(),
                input: ProbeInput::File(PathBuf::from("feature.m2ts")),
                size_hint: None,
                size_hint_is_estimate: false,
                segment_count: Some(1),
                segment_map: Some("00800.mpls".to_owned()),
                chapters: None,
                duplicate_key: None,
                stream_languages: HashMap::new(),
                authored_stream_pids: None,
                is_main_title: false,
            },
            probe,
        )
        .unwrap();

        assert_eq!(title.name, "Example Feature");
        assert_eq!(title.duration_ms, 7_445_500);
        assert_eq!(title.size_bytes, Some(27_487_790_694));
        assert!(!title.size_is_estimate);
        assert_eq!(title.tracks.len(), 3);
        assert!(
            title.tracks[2]
                .properties
                .contains(&"forced only".to_owned())
        );
        assert_eq!(title.chapters.len(), 2);
    }

    #[test]
    fn estimates_size_from_bit_rate_when_the_input_has_no_file_size() {
        let candidate = ProbeCandidate {
            id: 0,
            source_title_id: "1".to_owned(),
            disc_title_number: None,
            display_name: "Title 1".to_owned(),
            input: ProbeInput::Dvd {
                root: PathBuf::from("/disc"),
                title: 1,
            },
            size_hint: None,
            size_hint_is_estimate: true,
            segment_count: None,
            segment_map: None,
            chapters: None,
            duplicate_key: None,
            stream_languages: HashMap::new(),
            authored_stream_pids: None,
            is_main_title: false,
        };
        let probe: ProbeOutput = serde_json::from_str(
            r#"{
                "streams": [{"index":0,"codec_type":"video","codec_name":"mpeg2video"}],
                "format": {"duration":"120.0","bit_rate":"8000000"}
            }"#,
        )
        .unwrap();

        let title = title_from_probe(&candidate, probe).unwrap();

        assert_eq!(title.size_bytes, Some(120_000_000));
        assert!(title.size_is_estimate);
    }

    #[test]
    fn fills_missing_bluray_languages_from_stream_pids() {
        let mut stream_languages = HashMap::new();
        stream_languages.insert(0x1100, "eng".to_owned());
        stream_languages.insert(0x1200, "jpn".to_owned());
        let candidate = ProbeCandidate {
            id: 0,
            source_title_id: "00014".to_owned(),
            disc_title_number: None,
            display_name: "Feature".to_owned(),
            input: ProbeInput::File(PathBuf::from("feature.m2ts")),
            size_hint: None,
            size_hint_is_estimate: false,
            segment_count: Some(1),
            segment_map: Some("00001.m2ts".to_owned()),
            chapters: None,
            duplicate_key: None,
            stream_languages,
            authored_stream_pids: None,
            is_main_title: false,
        };
        let probe: ProbeOutput = serde_json::from_str(
            r#"{
                "streams": [
                    {"index":0,"id":"0x1100","codec_type":"audio","codec_name":"dts"},
                    {"index":1,"id":"0x1200","codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"}
                ],
                "format": {"duration":"10.0"}
            }"#,
        )
        .unwrap();

        let title = title_from_probe(&candidate, probe).unwrap();

        assert_eq!(title.tracks[0].language.as_deref(), Some("eng"));
        assert_eq!(title.tracks[1].language.as_deref(), Some("jpn"));
    }

    #[test]
    fn collapses_repeated_bluray_audio_and_subtitle_pids() {
        let candidate = ProbeCandidate {
            id: 0,
            source_title_id: "00014".to_owned(),
            disc_title_number: None,
            display_name: "Feature".to_owned(),
            input: ProbeInput::BluRay {
                root: PathBuf::from("/disc"),
                playlist: Some(14),
            },
            size_hint: None,
            size_hint_is_estimate: false,
            segment_count: Some(2),
            segment_map: Some("00001.m2ts, 00002.m2ts".to_owned()),
            chapters: None,
            duplicate_key: None,
            stream_languages: HashMap::new(),
            authored_stream_pids: None,
            is_main_title: false,
        };
        let probe: ProbeOutput = serde_json::from_str(
            r#"{
                "streams": [
                    {"index":0,"id":"0x1011","codec_type":"video","codec_name":"h264"},
                    {"index":1,"id":"0x1100","codec_type":"audio","codec_name":"dts"},
                    {"index":2,"id":"0x1101","codec_type":"audio","codec_name":"ac3"},
                    {"index":3,"id":"0x1101","codec_type":"audio","codec_name":"ac3"},
                    {"index":4,"id":"0x1200","codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"},
                    {"index":5,"id":"0x1200","codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"}
                ],
                "format": {"duration":"10.0"}
            }"#,
        )
        .unwrap();

        let title = title_from_probe(&candidate, probe).unwrap();

        assert_eq!(
            title
                .tracks
                .iter()
                .map(|track| track.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 4]
        );
    }

    #[test]
    fn hides_physical_bluray_streams_not_authored_into_the_playlist() {
        let candidate = ProbeCandidate {
            id: 0,
            source_title_id: "00026".to_owned(),
            disc_title_number: None,
            display_name: "Main feature".to_owned(),
            input: ProbeInput::BluRay {
                root: PathBuf::from("/disc"),
                playlist: Some(26),
            },
            size_hint: None,
            size_hint_is_estimate: false,
            segment_count: Some(1),
            segment_map: Some("00105.m2ts".to_owned()),
            chapters: None,
            duplicate_key: None,
            stream_languages: HashMap::new(),
            authored_stream_pids: Some(HashSet::from([0x1011, 0x1100, 0x1101, 0x1200])),
            is_main_title: true,
        };
        let probe: ProbeOutput = serde_json::from_str(
            r#"{
                "streams": [
                    {"index":0,"id":"0x1011","codec_type":"video","codec_name":"h264"},
                    {"index":1,"id":"0x1100","codec_type":"audio","codec_name":"dts"},
                    {"index":2,"id":"0x1101","codec_type":"audio","codec_name":"ac3"},
                    {"index":3,"id":"0x1102","codec_type":"audio","codec_name":"ac3"},
                    {"index":9,"id":"0x1200","codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"}
                ],
                "format": {"duration":"10.0"}
            }"#,
        )
        .unwrap();

        let title = title_from_probe(&candidate, probe).unwrap();

        assert!(title.is_main_title);
        assert_eq!(
            title
                .tracks
                .iter()
                .map(|track| track.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 9]
        );
    }

    #[test]
    fn preserves_bluray_tracks_with_matching_metadata_but_different_pids() {
        let candidate = ProbeCandidate {
            id: 0,
            source_title_id: "00014".to_owned(),
            disc_title_number: None,
            display_name: "Feature".to_owned(),
            input: ProbeInput::BluRay {
                root: PathBuf::from("/disc"),
                playlist: Some(14),
            },
            size_hint: None,
            size_hint_is_estimate: false,
            segment_count: Some(1),
            segment_map: Some("00001.m2ts".to_owned()),
            chapters: None,
            duplicate_key: None,
            stream_languages: HashMap::new(),
            authored_stream_pids: None,
            is_main_title: false,
        };
        let probe: ProbeOutput = serde_json::from_str(
            r#"{
                "streams": [
                    {"index":0,"id":"0x1101","codec_type":"audio","codec_name":"ac3","channels":2},
                    {"index":1,"id":"0x1102","codec_type":"audio","codec_name":"ac3","channels":2}
                ],
                "format": {"duration":"10.0"}
            }"#,
        )
        .unwrap();

        let title = title_from_probe(&candidate, probe).unwrap();

        assert_eq!(title.tracks.len(), 2);
        assert_eq!(title.tracks[0].summary(), title.tracks[1].summary());
    }

    #[test]
    fn prefers_bluray_chapters_over_ffprobe_chapters() {
        let candidate = ProbeCandidate {
            id: 0,
            source_title_id: "00014".to_owned(),
            disc_title_number: None,
            display_name: "Feature".to_owned(),
            input: ProbeInput::File(PathBuf::from("feature.m2ts")),
            size_hint: None,
            size_hint_is_estimate: false,
            segment_count: Some(1),
            segment_map: Some("00001.m2ts".to_owned()),
            chapters: Some(vec![Chapter {
                index: 7,
                start_ms: 1_000,
                end_ms: 2_000,
                name: None,
            }]),
            duplicate_key: None,
            stream_languages: HashMap::new(),
            authored_stream_pids: None,
            is_main_title: false,
        };
        let probe: ProbeOutput = serde_json::from_str(
            r#"{
                "streams": [{"index":0,"codec_type":"video","codec_name":"h264"}],
                "chapters": [],
                "format": {"duration":"10.0"}
            }"#,
        )
        .unwrap();

        let title = title_from_probe(&candidate, probe).unwrap();

        assert_eq!(title.chapters.len(), 1);
        assert_eq!(title.chapters[0].index, 7);
        assert_eq!(title.chapters[0].start_ms, 1_000);
    }

    #[test]
    fn estimates_size_from_a_bounded_packet_sample() {
        let sample: PacketProbeOutput = serde_json::from_str(
            r#"{
                "packets": [
                    {"pts_time":"10.0","size":"5000000"},
                    {"pts_time":"20.0","size":"5000000"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(size_from_packet_sample(&sample, 120_000), Some(120_000_000));
    }
}
