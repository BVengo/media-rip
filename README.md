# MediaRip

MediaRip is a native Rust desktop application for inspecting optical media, disc folders, and
ordinary media files, then preserving or encoding selected tracks. Its desktop workflow uses
source selection, a hierarchical item/track panel, context-sensitive information, output format
and processing controls, progress, messages, and status. DVD, Blu-ray, and secure Audio CD reading
use the same workflow.

It is not a wrapper around MakeMKV. The project never invokes, links to, or requires `makemkv` or
`makemkvcon`.

## What works

- Native GTK 4 UI written entirely in Rust; no Tauri, browser, HTML, JavaScript, or web view.
- Linux optical-drive discovery with mounted-disc labels and device information.
- DVD title discovery from `VIDEO_TS.IFO`.
- Blu-ray playlist discovery with disc-defined main-title marking and authored-stream filtering.
- Audio CD track discovery, CD-TEXT naming/tagging, and secure extraction for music and audiobooks.
- Video, audio, subtitle, attachment, chapter, duration, size, language, and codec inspection.
- Per-title and per-track selection with multilingual audio and subtitle defaults.
- Lossless stream-copy remuxing into MKV through the open-source FFmpeg toolchain.
- Optional H.264/MP4, H.265/MKV, AV1/WebM, FLAC, WAV, Opus, and AAC/M4A output profiles.
- Live title progress, byte count, estimated rate/time remaining, cancellation, and safe partial
  output handling.
- Editable per-title output names and whole-job destination preflight before writing begins.
- Media-file inputs, useful both directly and for developing without an optical drive.

Lossless MKV remains the default. Encoding is always an explicit profile, and incompatible selected
tracks are rejected before output begins rather than silently dropped.

## Build and run

Linux packages are named differently by distribution, but a development machine needs:

- Rust 1.85 or newer
- GTK 4 development files
- libdvdread and libbluray development files
- FFmpeg and FFprobe
- cdparanoia for Audio CDs; cd-info (from libcdio-utils) is optional for CD-TEXT
- an FFmpeg build with the `dvdvideo` demuxer and `bluray` protocol for optical media
- `lsblk`, `udevadm`, and optionally `eject`

On Debian-family systems, the starting point is typically:

```sh
sudo apt install build-essential libgtk-4-dev libdvdread-dev libbluray-dev ffmpeg cdparanoia libcdio-utils util-linux udev eject
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
