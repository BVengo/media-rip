# MediaRip

MediaRip is a Linux desktop application built with Rust and GTK4 for inspecting and
ripping DVDs, Blu-rays, Audio CDs, disc folders and media files. Select tracks, review
metadata, then preserve or encode them in your chosen output format.

## Features

- Optical drive, DVD title, Blu-ray playlist and Audio CD track discovery
- Track selection, artwork and metadata editing for films, TV, music and audiobooks
- Automated disc reading, tagging, ripping and ejection
- Metadata from [MusicBrainz](https://musicbrainz.org),
  [TheDiscDB](https://thediscdb.com), [TheTVDB](https://thetvdb.com/) and
  [TVMaze](https://www.tvmaze.com/)
- Push notifications through [ntfy](https://ntfy.sh/)

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

## Keyboard shortcuts

All quick actions are available from the menus, which also show their shortcuts.

| Action | Shortcut |
| --- | --- |
| Open files | Ctrl+O |
| Open folder | Ctrl+Shift+O |
| Open disc | Ctrl+D |
| Eject disc | Ctrl+E |
| Refresh drives | F5 |
| Settings | Ctrl+, |
| Cancel current operation | Esc |
| Quit | Ctrl+Q |

## Disc support

Unencrypted DVDs and Blu-rays are supported. Encrypted disc support depends on the
libraries and keys installed for your system's FFmpeg, libdvdread and libbluray stack.
MediaRip does not download keys or bypass protection. Use it only with media you are
legally permitted to copy in your jurisdiction.

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

## Licence


MediaRip is licensed under GPL-3.0-or-later. See [LICENSE](LICENSE). Licences
and copyright notices for dependencies included in the executable are in
[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
