use std::{path::PathBuf, process::Command};

use media_rip_core::{DiscKind, Drive, MediaEngineError};
use serde::Deserialize;

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

    let mut drives = devices
        .blockdevices
        .into_iter()
        .filter(|device| device.device_type == "rom")
        .map(|device| {
            let udev_properties = udev_properties(&device.path);
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

            Drive {
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
            }
        })
        .collect::<Vec<_>>();

    drives.sort_by(|left, right| left.device_path.cmp(&right.device_path));
    Ok(drives)
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
}
