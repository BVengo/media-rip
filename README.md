# MediaRip

MediaRip is a native Rust desktop application for inspecting optical media, disc folders, and
ordinary media files, then preserving or encoding selected tracks. Its desktop workflow uses
source selection, a hierarchical item/track panel, context-sensitive information, output format
and processing controls, progress, messages, and status. DVD, Blu-ray, and secure Audio CD reading
use the same workflow.


## AI Disclosure
Development of this project is being done with heavy usage of AI.

It was designed as a personal project first, to address the frustrations I had with my current
setup, requiring many different pieces of software and general lack of automation where it should
be possible.
- DVDs and BluRays
- Audiobooks
- CDs
- Metadata analysis / gathering

I wanted a complete yet quick solution to these problems, since I don't have much free time to
develop one from scratch. At some stage I do intend on going through and thoroughly cleaning up the
codebase, fully rewriting anything that the AI did suboptimally, however **today is not that day**.
Treat this as what it is - a personal project that I decided to open source!


## Features

- **Linux** only
- GTK4 UI in Rust
- Optical drive discovery
- Blu-Ray playlist and main title discovery
- Audio CD track discovery
- Automations (hands-off read, tag, rip, eject)
- Integrations (listed below) for more features

**Integrations**
- [MusicBrainz](https://musicbrainz.org) for audio cd tagging
- [TheDiscDB](https://thediscdb.com) for DVD/Blu-ray fingerprint matches
- [TheTVDB](https://thetvdb.com/) for tv/movie metadata
- [TVMaze](https://www.tvmaze.com/) for tv/movie metadata
- [ntfy](https://ntfy.sh/) for push notifications

With these integrations come:
- Track inspection
- Artworks
- Metadata review


## Settings

Settings are grouped by task: Output, Selection, Metadata, Disc automation and Completion.
Search finds controls across all five pages. Selection, metadata and format defaults apply
when opening a source; use the workflow to change the current job.

Output keeps all destination overrides together and separates video and audio formats.
"Ask every time" leaves the destination blank even when an override is saved. Short-title
and playlist preferences change the initial selection; they never hide discovered titles.

Online metadata access controls metadata providers independently of ntfy notifications.
Automatic disc reading is armed by the first manual read in each session. Automatic saving
pauses when metadata needs review; after reviewing it, start the job from Output.

Settings changes are validated before saving. Older working settings are migrated; obsolete
placeholder options are no longer displayed or written. Use `make test-ui` to exercise the GTK
workflows with Xvfb. Set `MEDIA_RIP_SETTINGS_SCREENSHOTS` to a directory when running that test
to capture the rendered settings pages.


## Metadata editing

The Metadata tab uses content-specific fields. Video titles can be classified as films,
television or video/extras independently of whether the source is DVD, Blu-ray or a file.
TV fields include series, season and episode, including season 0 for specials, and can be
edited without an online match. Music uses album and artist fields; audiobooks use book,
author, narrator, book series and chapter fields. Changing content type preserves existing tags.
Manual film titles and release dates use the configured movie filename template. The
same applies to an unidentified main feature once its title is entered. An
explicitly edited output filename is retained when metadata changes. During ripping,
workflow tabs remain available for review while editing controls are locked. Ejecting
a disc returns the workflow to Identify.

"Apply to selected" shares a field with selected items of the same content type. Track,
chapter and episode titles and numbers remain individual. The source-wide audio type chooser
still explicitly applies to every track. Provider proposals remain available for review.

The workflow keeps the source and content type visible, with Back and Continue controls
between selection, metadata and output. Confirmed films and extras suppress TV assignment
controls, even when an older series context remains. Audio-only files use audio output
formats just like CDs. Optional audio numbering and release fields expand on demand.
Set `MEDIA_RIP_WORKFLOW_SCREENSHOTS` to a directory with `make test-ui` to capture the
film, music and audiobook metadata layouts.


## DVD title review

DVD titles are playback paths, not necessarily separate films or copies of the video.
MediaRip analyses chapter and cell tables in authored order, checks sector addresses against
VOB extents, and falls back to BUP tables when the corresponding IFO is unreadable. It retains
the original title numbers and uses the same chapter boundaries for probing and ripping.
Padding-only boundary chapters are excluded; cell-derived sizes are shown as estimates.

A DVD feature lasting at least 40 minutes is marked as the main title when its duration
is at least twice the next longest path and exceeds the combined duration of the other
distinct, valid playback paths. Known
TV discs and ambiguous collections keep their existing defaults. The "Select only the
main title" setting applies to DVDs and Blu-rays; the saved configuration retains the
existing `bluray.select_only_main_title` key.

Duplicate paths and structurally suspect titles start unchecked in a separate review group.
The title details show the VTS location, cells, padding and validation concerns. Suspect paths
are excluded from automatic metadata matching. This structural analysis does not prove that
a path is the intended feature, and does not implement a DVD menu virtual machine or promise
MakeMKV-equivalent handling of every authored protection scheme.

A remembered TV identity is only reused when the current disc's independently obtained title
matches the series. An opaque label or a failed lookup leaves the source unidentified.


## Disc support and encryption

Unencrypted DVD and Blu-ray media are supported through FFmpeg's open DVD/Blu-ray integrations.

Commercial discs commonly use CSS, AACS, or BD+. Whether an encrypted disc can be read depends
on the legally installed open-source libraries and keys available to the system FFmpeg /
libdvdread / libbluray stack. MediaRip does not download keys or attempt to bypass protection.

Use MediaRip only with media that you are legally permitted to copy in your jurisdiction.

## Dev Commands

Some useful commands during development.

```sh
make check
make test
make run
make release
```

## Build and run

Linux packages are named differently by distribution, but a development machine needs:

- Rust 1.88 or newer
- GTK 4 development files
- libdvdread development files; libbluray is optional at runtime for folders and required for raw Blu-ray devices
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

## Licence

MediaRip is licensed under GPL-3.0-or-later. See [LICENSE](LICENSE). Licences
and copyright notices for dependencies included in the executable are in
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
