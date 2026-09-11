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

- Rust 1.85 or newer
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
