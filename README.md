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
- DVD/Blu-ray authored-metadata reading plus independently configurable exact TheDiscDB fingerprint
  matching for movies, episodes, and extras. TVMaze plus optional TMDB and TVDB enrichment adds canonical
  identities, episode ordering, poster artwork, and output tags. When both are configured they are
  queried concurrently and agreement is retained as stronger identity evidence.
- Audio CD track discovery, enabled-by-default exact MusicBrainz matching with a validated
  CD-TEXT release-search fallback, editable output tags,
  Cover Art Archive artwork embedding, configurable matched-track names (default
  `## - Song Name`), CD-TEXT fallback, and secure extraction for music and audiobooks.
- Video, audio, subtitle, attachment, chapter, duration, size, language, and codec inspection.
- Per-title and per-track selection with multilingual audio and subtitle defaults.
- Lossless stream-copy remuxing into MKV through the open-source FFmpeg toolchain.
- Optional H.264/MP4, H.265/MKV, AV1/WebM, FLAC, WAV, Opus, and AAC/M4A output profiles.
- Live title progress, byte count, estimated rate/time remaining, cancellation, and safe partial
  output handling.
- Resumable multi-title jobs: completed outputs stay in place while unfinished items remain selected
  after a failure or cancellation.
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

Online Audio CD tagging uses the public MusicBrainz web service. MediaRip identifies itself,
limits lookups to one request per second, and does not poll in the background. Distributors and
commercial deployments remain responsible for complying with the
[MusicBrainz API terms and rate limits](https://musicbrainz.org/doc/MusicBrainz_API).

When enabled, video-disc matching sends only a fingerprint derived from sorted disc-file sizes to TheDiscDB;
no audio or video is uploaded. Exact matches are checked again against each scanned title's
duration and size before names are applied. Successful responses and artwork are cached under the
platform cache directory. TheDiscDB and TVMaze have independent settings and need no keys. Provider enrichment is optional: enter a TMDB v4
API Read Access Token, a TVDB v4 project API key, or both under **Settings → Integrations → Video
metadata**. A TVDB subscriber PIN is needed only for user-supported project keys. When enabled, the
credential-free TVMaze API is queried for TV series alongside configured providers. Season-labelled
TV discs use the same conservative probable-title review gate for every provider. Provider requests
run concurrently; conflicting title-only results are not applied automatically. Review the
[TMDB API terms](https://www.themoviedb.org/api-terms-of-use),
[TVDB API licensing](https://thetvdb.com/api-information), and
[TVMaze API licensing](https://www.tvmaze.com/api) before enabling online metadata.

If an authored disc label is not a usable title and TheDiscDB has no exact record, open the disc,
paste its public TVMaze show URL or TheTVDB series/movie URL into **Metadata link**, and choose
**Identify**. MediaRip detects the provider from that single URL and reads the source again using
the selected identity instead of guessing from the label. TheTVDB links use the configured TVDB
API key; TVMaze links need no credentials.

After a DVD with applied metadata is successfully saved, MediaRip remembers that identity and the
next episode number. With **Use previous DVD match** enabled, a later DVD is retried against that
identity only when authored metadata, TheDiscDB, and the normal provider searches find nothing.
Manual metadata links and first-episode adjustments are included in the remembered state. Disable
the setting under **Settings → Integrations → Video metadata** to keep each disc independent.

This product uses the TMDB API but is not endorsed or certified by TMDB.

[Metadata provided by TheTVDB](https://thetvdb.com/). Please consider adding missing information or
subscribing. TVDB API access and artwork use remain subject to TVDB's licensing terms.

[TV metadata provided by TVMaze](https://www.tvmaze.com/). TVMaze data is available under CC BY-SA.

When a TV disc has no exact TheDiscDB record, MediaRip can use an authored season/disc label, DVD
play-all structure, and configured-provider episode order to show lower-confidence episode
suggestions. TVDB's DVD order is preferred when available; TMDB and TVMaze can fill missing runtimes only for
episodes whose names agree. Suggestions are runtime-aligned when the resulting per-episode data can
distinguish a unique season window. The first-episode
field can remap an ambiguous proposal to the correct consecutive range during review, and that
correction is remembered for the exact disc fingerprint. Suggestions are never applied automatically: review each suggestion in the
content list and use **Apply** or **Apply all** explicitly.

## Completion notifications

MediaRip sends a desktop system notification after a rip finishes successfully without raising the
application window. Under **Settings → Drives & I/O → Completion**, it can also independently play
the desktop's standard alert sound and publish a notification to an ntfy topic.
For ntfy, enter the server URL and the same topic subscribed to by the phone app. A self-hosted
server on the local network can use a URL such as `http://192.168.1.20:8080`; the hosted service is
`https://ntfy.sh`. Protected topics can use an optional ntfy access token. Choose an unguessable
topic name when using a public server.

Notification delivery runs in the background and never changes a successful rip into a failed one.
A delivery problem is instead written as a warning in the activity log.

Useful project commands:

```sh
make check
make test
make run
make release
```

## Debug metadata logs

Debug builds write detailed diagnostics to standard error by default. Each scan includes the complete
JSON returned by ffprobe, TheDiscDB, MusicBrainz, TMDB, TVDB, and TVMaze; raw optical-drive, Audio CD
TOC, and CD-TEXT discovery output; every authored probe candidate; and a final normalized snapshot of
the disc, titles, chapters, tracks, tags, provider matches, and suggestions. Raw JSON is logged before
MediaRip deserializes it, so fields the application does not currently use remain visible. Cached API
responses are identified as `cache` or `stale-cache` in the log.

Capture a debug run with:

```sh
MEDIA_RIP_LOG=debug cargo run -p media-rip 2>media-rip-debug.log
```

For manual TV-episode alignment work, set `MEDIA_RIP_EPISODE_REPORT_DIR` to write spreadsheet-ready
CSV files after a show and season episode list have been resolved:

```sh
MEDIA_RIP_LOG=debug MEDIA_RIP_EPISODE_REPORT_DIR=logs cargo run -p media-rip
```

`make run LOG=1` enables both the debug log and the TV export feature automatically. Files are written
only when a TV series and its episode list have been selected; movie and unresolved-video scans stay
in the normal debug log. Each qualifying TV scan writes the original API responses to `raw-api.json`
before typed deserialization, plus a `raw-fields.csv` view which represents every JSON leaf by its
JSON Pointer. These are the canonical diagnostics for fields MediaRip does not currently understand;
the remaining normalized JSON and CSV files are derived views. The derived files cover disc titles,
selected episodes, per-provider episodes, and every title-by-episode pairing. The alignment file
contains rejected titles, candidate-filter reasons, runtime deltas, percentage error, sequence
offset, and the current match. The provider file highlights episodes absent from the reconciled
selected list or differing in name or runtime.

`MEDIA_RIP_LOG=info` disables the payload dumps, and a release build defaults to that quieter level.
Debug logs can contain local paths, drive serial numbers, disc labels, and metadata search terms. API
keys, subscriber PINs, bearer tokens, and binary artwork bytes are not logged; artwork MIME type and
byte length are included instead.

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
