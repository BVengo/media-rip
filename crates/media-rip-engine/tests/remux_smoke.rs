use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};

use media_rip_core::{
    EngineEvent, MediaEngine, OutputProfile, RipRequest, ports::CancellationToken,
};
use media_rip_engine::{OpenMediaEngine, tool_availability};

#[test]
fn scans_and_remuxes_a_small_media_source() {
    let tools = tool_availability();
    if !tools.ffmpeg || !tools.ffprobe {
        eprintln!("skipping remux smoke test because FFmpeg tools are not installed");
        return;
    }

    let temp = TestDirectory::new();
    let input = temp.path().join("source.mp4");
    let output = temp.path().join("output");
    let generated = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x90:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "0.5",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-y",
        ])
        .arg(&input)
        .status()
        .expect("ffmpeg should start");
    assert!(generated.success(), "ffmpeg should create the fixture");

    let engine = OpenMediaEngine::new();
    let (scan_tx, scan_rx) = mpsc::channel();
    engine.scan(input, scan_tx, CancellationToken::default());
    let catalog = scan_rx
        .try_iter()
        .find_map(|event| match event {
            EngineEvent::ScanFinished(catalog) => Some(*catalog),
            _ => None,
        })
        .expect("the generated media should scan");
    assert_eq!(catalog.titles.len(), 1);
    assert_eq!(catalog.titles[0].tracks.len(), 2);

    let request = RipRequest::from_catalog(catalog.clone(), output.clone(), false);
    let (rip_tx, rip_rx) = mpsc::channel();
    engine.rip(request, rip_tx, CancellationToken::default());
    assert!(
        rip_rx
            .try_iter()
            .any(|event| matches!(event, EngineEvent::RipFinished { .. }))
    );

    let result = output.join("source_t00.mkv");
    assert!(result.is_file(), "the MKV should be finalized");
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name",
            "-of",
            "csv=p=0",
        ])
        .arg(result)
        .stdout(Stdio::piped())
        .output()
        .expect("ffprobe should inspect the result");
    assert!(probe.status.success());
    assert!(String::from_utf8_lossy(&probe.stdout).contains("matroska"));

    if encoder_available("libx264") && encoder_available("aac") {
        let request = RipRequest::from_catalog_with_profile(
            catalog,
            output.clone(),
            false,
            OutputProfile::H264Mp4,
        );
        let (encode_tx, encode_rx) = mpsc::channel();
        engine.rip(request, encode_tx, CancellationToken::default());
        assert!(
            encode_rx
                .try_iter()
                .any(|event| matches!(event, EngineEvent::RipFinished { .. }))
        );
        assert!(
            output.join("source_t00.mp4").is_file(),
            "the encoded MP4 should be finalized"
        );
    }
}

fn encoder_available(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .is_ok_and(|output| {
            String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                line.split_whitespace()
                    .nth(1)
                    .is_some_and(|encoder| encoder == name)
            })
        })
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "media-rip-remux-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
