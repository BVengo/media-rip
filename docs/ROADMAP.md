# Roadmap

## Hardware validation

- Build a reproducible test matrix across authored DVD-Video, Blu-ray, UHD, folders, and images.
- Improve raw Blu-ray enumeration without requiring a desktop mount.
- Detect drive changes and media insertion through native system notifications.
- Add read-error retry strategy, drive speed, and bad-sector reporting.

## Ripping depth

- Parse Blu-ray MPLS play items and segment maps directly.
- Detect duplicate and suspicious playlists without hiding them.
- Improve angle handling, seamless branching, and forced-subtitle detection.
- Add output filename editing and reusable naming templates.
- Add whole-disc backup as a separate job type.

## Formats and encoding

- Introduce output profiles while retaining stream-copy MKV as the safe default.
- Add optional AV1, H.265, H.264, Opus, AAC, and FLAC encoding through a codec provider.
- Add MP4 and WebM muxers where selected streams are compatible.
- Add audio-CD discovery, secure extraction, verification, and tagging.

## Platforms and distribution

- Add native drive providers for Windows and macOS.
- Produce Flatpak, AppImage, Debian, and RPM packages.
- Add accessibility and keyboard-navigation passes plus translated UI strings.
