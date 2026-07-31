# MediaRip

MediaRip is a native Rust desktop application for inspecting DVDs, Blu-rays, disc folders, and
ordinary media files, then preserving selected streams in Matroska (`.mkv`) files. Its desktop
workflow intentionally follows the familiar MakeMKV layout: source selection, a hierarchical
title/track panel, context-sensitive information, output-folder controls, a large **Make MKV**
action, progress, messages, and status.

It is not a wrapper around MakeMKV. The project never invokes, links to, or requires `makemkv` or
`makemkvcon`.

## What works

- Native GTK 4 UI written entirely in Rust; no Tauri, browser, HTML, JavaScript, or web view.
- Linux optical-drive discovery with mounted-disc labels and device information.
- DVD title discovery from `VIDEO_TS.IFO`.
- Blu-ray playlist discovery with disc-defined main-title marking and authored-stream filtering.
- Video, audio, subtitle, attachment, chapter, duration, size, language, and codec inspection.
- Per-title and per-track selection with multilingual audio and subtitle defaults.
- Lossless stream-copy remuxing into MKV through the open-source FFmpeg toolchain.
- Live title progress, byte count, estimated rate/time remaining, cancellation, and safe partial
  output handling.
- Media-file inputs, useful both directly and for developing without an optical drive.

The current release is deliberately a remuxer: selected streams are copied without re-encoding.
Encoding presets and additional output containers fit behind the existing engine contracts and are
planned rather than mixed into the initial ripping path.

## Build and run

Linux packages are named differently by distribution, but a development machine needs:

- Rust 1.85 or newer
- GTK 4 development files
- libdvdread and libbluray development files
- FFmpeg and FFprobe
- an FFmpeg build with the `dvdvideo` demuxer and `bluray` protocol for optical media
- `lsblk`, `udevadm`, and optionally `eject`

On Debian-family systems, the starting point is typically:

```sh
sudo apt install build-essential libgtk-4-dev libdvdread-dev libbluray-dev ffmpeg util-linux udev eject
```

Then:

```sh
cargo run -p media-rip
```

Open a source immediately from the command line:

```sh
cargo run -p media-rip -- /path/to/media-or-disc-folder
```

Useful project commands:

```sh
make check
make test
make run
make release
```

## Disc support and encryption

MediaRip ships no proprietary code, decryption keys, MakeMKV components, or calls to MakeMKV.
Unencrypted DVD and Blu-ray media are supported through FFmpeg's open DVD/Blu-ray integrations.

Commercial discs commonly use CSS, AACS, or BD+. Whether an encrypted disc can be read currently
depends on the legally installed open-source libraries and keys available to the system FFmpeg /
libdvdread / libbluray stack. MediaRip does not download keys or attempt to bypass protection.
See [media support](docs/MEDIA_SUPPORT.md) for the exact current boundary.

Use MediaRip only with media you are legally permitted to copy in your jurisdiction.

## Repository layout

```text
crates/
├── media-rip-core/      domain models, formatting, engine contracts
├── media-rip-engine/    drive discovery, DVD/BD probing, FFmpeg remuxing
└── media-rip-native/    GTK application, controller, reusable UI components
docs/                    architecture, media support, roadmap
packaging/linux/         desktop integration and application metadata
tests/fixtures/          stable probe fixtures shared by engine tests
```

The dependency direction is one-way: the native application depends on the engine and core, the
engine depends on core, and core has no knowledge of either implementation. Read
[the architecture guide](docs/ARCHITECTURE.md) before adding a format or encoder.

## Project status

This is the first working foundation, not a claim of feature parity with MakeMKV's proprietary
decryption engine. The UI and primary remux workflow are in place; hardware/disc testing across
different drives and authored media is the next practical milestone.

MakeMKV is a trademark of its respective owner. MediaRip is an independent project and is not
affiliated with or endorsed by MakeMKV.
