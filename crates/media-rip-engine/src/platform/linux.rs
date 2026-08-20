use std::{path::PathBuf, process::Command, sync::Mutex};

use media_rip_core::{DiscKind, Drive, MediaEngineError};
use serde::Deserialize;

use crate::diagnostics;

static LAST_LOGGED_DRIVES: Mutex<Option<Vec<Drive>>> = Mutex::new(None);

#[derive(Debug, Deserialize)]
struct Lsblk {
    #[serde(default)]
    blockdevices: Vec<BlockDevice>,
}

#[derive(Debug, Deserialize)]
struct BlockDevice {
    name: String,
    path: PathBuf,
    #[serde(rename = "type")]
    device_type: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    rev: Option<String>,
    #[serde(default)]
    serial: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    mountpoint: Option<PathBuf>,
}

pub fn discover_optical_drives() -> Result<Vec<Drive>, MediaEngineError> {
    let output = Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--nodeps",
            "--include",
            "11",
            "--output",
            "NAME,PATH,TYPE,MODEL,VENDOR,REV,SERIAL,LABEL,FSTYPE,SIZE,MOUNTPOINT",
        ])
        .output()
        .map_err(|error| MediaEngineError::Process(format!("could not run lsblk: {error}")))?;

    if !output.status.success() {
        return Err(MediaEngineError::Process(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }

    let devices: Lsblk = serde_json::from_slice(&output.stdout)
        .map_err(|error| MediaEngineError::Process(format!("invalid lsblk output: {error}")))?;

    let mut discovered = devices
        .blockdevices
        .into_iter()
        .filter(|device| device.device_type == "rom")
        .map(|device| {
            let udev_properties = udev_properties(&device.path);
            let device_path = device.path.clone();
            let media_kind = device
                .mountpoint
                .as_deref()
                .map(crate::source::detect_kind)
                .unwrap_or(DiscKind::Unknown);
            let display_name = join_nonempty([
                device.vendor.as_deref(),
                device.model.as_deref(),
                Some(device.name.as_str()),
            ]);
            let media_kind = if media_kind == DiscKind::Unknown {
                media_kind_from_udev(udev_properties.as_deref().unwrap_or_default())
            } else {
                media_kind
            };
            let media_present = device.mountpoint.is_some()
                || device
                    .label
                    .as_ref()
                    .is_some_and(|label| !label.trim().is_empty())
                || udev_properties.as_deref().is_some_and(|properties| {
                    properties.lines().any(|line| line == "ID_CDROM_MEDIA=1")
                });

            let drive = Drive {
                device_path: device.path,
                mount_path: device.mountpoint,
                display_name,
                media_present,
                vendor: clean(device.vendor),
                model: clean(device.model),
                revision: clean(device.rev),
                serial: clean(device.serial),
                bus: Some("Optical".to_owned()),
                media_kind,
                media_label: clean(device.label),
                capacity_bytes: device.size.filter(|size| *size > 0),
            };
            (drive, (device_path, udev_properties))
        })
        .collect::<Vec<_>>();

    discovered.sort_by(|left, right| left.0.device_path.cmp(&right.0.device_path));
    let (drives, udev_snapshots): (Vec<_>, Vec<_>) = discovered.into_iter().unzip();
    log_drive_snapshot(&drives, &output.stdout, &udev_snapshots);

    Ok(drives)
}

fn log_drive_snapshot(
    drives: &[Drive],
    lsblk_output: &[u8],
    udev_snapshots: &[(PathBuf, Option<String>)],
) {
    let changed = LAST_LOGGED_DRIVES
        .lock()
        .map(|mut previous| replace_if_changed(&mut previous, drives))
        .unwrap_or(true);
    if !changed {
        log::trace!(
            target: "media_rip_engine::drive_monitor",
            "optical drive poll completed; normalized state unchanged, raw payload suppressed"
        );
        return;
    }

    log::debug!(
        target: "media_rip_engine::drive_monitor",
        "optical drive state changed: {}",
        drive_summary(drives)
    );
    diagnostics::tool_json("lsblk", "optical drive state change", lsblk_output);
    for (path, properties) in udev_snapshots {
        if let Some(properties) = properties {
            diagnostics::text(
                "udevadm",
                &format!(
                    "device properties after state change for {}",
                    path.display()
                ),
                properties,
            );
        }
    }
}

fn replace_if_changed(previous: &mut Option<Vec<Drive>>, drives: &[Drive]) -> bool {
    if previous.as_deref() == Some(drives) {
        return false;
    }
    *previous = Some(drives.to_vec());
    true
}

fn drive_summary(drives: &[Drive]) -> String {
    if drives.is_empty() {
        return "no optical drives detected".to_owned();
    }
    drives
        .iter()
        .map(|drive| {
            format!(
                "{} present={} kind={} label={:?} mount={}",
                drive.device_path.display(),
                drive.media_present,
                drive.media_kind.display_name(),
                drive.media_label,
                drive
                    .mount_path
                    .as_deref()
                    .map_or_else(|| "none".to_owned(), |path| path.display().to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn udev_properties(path: &std::path::Path) -> Option<String> {
    let Ok(output) = Command::new("udevadm")
        .args(["info", "--query=property", "--name"])
        .arg(path)
        .output()
    else {
        return None;
    };
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn media_kind_from_udev(properties: &str) -> DiscKind {
    if properties.lines().any(|line| {
        matches!(
            line,
            "ID_CDROM_MEDIA_BD=1"
                | "ID_CDROM_MEDIA_BDR=1"
                | "ID_CDROM_MEDIA_BDRE=1"
                | "ID_CDROM_MEDIA_BDROM=1"
        )
    }) {
        DiscKind::BluRay
    } else if properties.lines().any(|line| {
        line.strip_prefix("ID_CDROM_MEDIA_TRACK_COUNT_AUDIO=")
            .and_then(|count| count.parse::<u32>().ok())
            .is_some_and(|count| count > 0)
    }) {
        DiscKind::AudioCd
    } else if properties
        .lines()
        .any(|line| line.starts_with("ID_CDROM_MEDIA_DVD") && line.ends_with("=1"))
    {
        DiscKind::Dvd
    } else {
        DiscKind::Unknown
    }
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn join_nonempty<'a>(parts: impl IntoIterator<Item = Option<&'a str>>) -> String {
    let value = parts
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if value.is_empty() {
        "Optical drive".to_owned()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_drive(label: Option<&str>) -> Drive {
        Drive {
            media_present: label.is_some(),
            media_label: label.map(str::to_owned),
            ..Drive::empty("/dev/sr0", "Test drive")
        }
    }

    #[test]
    fn identifies_common_inserted_disc_types_from_udev() {
        assert_eq!(
            media_kind_from_udev("ID_CDROM_MEDIA_DVD=1\n"),
            DiscKind::Dvd
        );
        assert_eq!(
            media_kind_from_udev("ID_CDROM_MEDIA_BDROM=1\n"),
            DiscKind::BluRay
        );
        assert_eq!(
            media_kind_from_udev("ID_CDROM_MEDIA_TRACK_COUNT_AUDIO=12\n"),
            DiscKind::AudioCd
        );
    }

    #[test]
    fn a_presence_property_alone_remains_unidentified_while_udev_settles() {
        assert_eq!(
            media_kind_from_udev("ID_CDROM_MEDIA=1\n"),
            DiscKind::Unknown
        );
    }

    #[test]
    fn raw_drive_diagnostics_are_selected_only_when_normalized_state_changes() {
        let empty = vec![test_drive(None)];
        let inserted = vec![test_drive(Some("SUPERNATURAL_S4_D5"))];
        let mut previous = None;

        assert!(replace_if_changed(&mut previous, &empty));
        assert!(!replace_if_changed(&mut previous, &empty));
        assert!(replace_if_changed(&mut previous, &inserted));
        assert!(!replace_if_changed(&mut previous, &inserted));
    }
}
