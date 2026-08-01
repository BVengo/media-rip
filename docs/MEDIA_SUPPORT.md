# Media support

| Input | Discovery | Rip output | Notes |
|---|---:|---:|---|
| Ordinary media file | Yes | Yes | Any input that the installed FFmpeg supports |
| Mounted DVD / `VIDEO_TS` folder | Yes | Yes | Enumerates titles from `VIDEO_TS.IFO` |
| Raw DVD device | Yes | Yes | Enumerates the title table through `libdvdread` |
| Mounted Blu-ray / `BDMV` folder | Yes | Yes | Main titles are marked; exact duplicate playlists can be grouped |
| Raw Blu-ray device | Yes | Yes | Main-title, playlist, chapter, and authored-stream metadata comes from `libbluray` |
| ISO image | Limited | Limited | Ordinary FFmpeg probing only in this release |
| Audio CD | Yes | Yes | One file per track; secure re-reading with cdparanoia and local CD-TEXT tagging |

## Output profiles

| Profile | Container | Processing |
|---|---|---|
| Preserve original tracks | MKV | Stream copy; safe default |
| H.264 + AAC | MP4 | Encodes video/audio and compatible text subtitles |
| H.265 | MKV | Encodes video; preserves other selected tracks |
| AV1 + Opus | WebM | Encodes video/audio and compatible text subtitles |
| Lossless audio | FLAC | Encodes one selected audio track |
| Uncompressed audio | WAV | Writes one selected audio track as 16-bit PCM |
| Compact audio | Opus | Encodes one selected audio track |
| Compatible audio | M4A | Encodes one selected audio track to AAC |

Profiles are checked against selected track types before a job starts. MP4 and WebM reject bitmap
subtitles, attachments, and data tracks rather than discarding them. Audio profiles require exactly
one selected audio track per item. The installed FFmpeg must provide the encoder named by the
profile; availability is checked before writing begins.

## Toolchain

DVD title tables and program-cell sector ranges are read through `libdvdread`; Blu-ray playlist
sizes, chapters, clip IDs, exact clip ranges, main-title identity, and authored stream PIDs are read
through `libbluray`. Title and track metadata is then obtained from FFprobe JSON and restricted to
the streams authored into each playlist. When authored size metadata is unavailable, MediaRip
samples 30 seconds of packets to estimate the title size. Output files are written by FFmpeg with
explicit stream maps, profile-selected codecs, and muxers. This is an open-source media stack and
contains no MakeMKV dependency.

Audio CD tables of contents are read with `cdparanoia`; embedded CD-TEXT can be read locally with
`cd-info` and no CDDB network request, or disabled in Settings. Each selected track is securely
extracted to temporary 16-bit, 44.1 kHz stereo PCM and then written by FFmpeg as FLAC, WAV, Opus,
or AAC/M4A. FLAC is the default for both music and audiobooks. Album, artist, title, and track/total
tags are written when the source metadata is available. Temporary PCM files are removed after
encoding, failure, or cancellation. Online checksum verification such as AccurateRip is not
implemented yet.

At startup, the Messages panel reports whether `ffprobe`, `ffmpeg`, and `cdparanoia` are available.
An absent required tool also produces a direct error before an operation starts.

## Protected media

No decryption keys or circumvention components are bundled. CSS-protected DVDs may be readable when
the user's legally installed `libdvdcss` is integrated with the system DVD stack. AACS/BD+
availability similarly depends on the user's libbluray/libaacs setup and lawful key configuration.
Failures are surfaced from the underlying open-source stack in the Messages panel.

Future decryption work should implement a dedicated engine capability and report:

- protection schemes detected;
- provider name and availability;
- whether a title can be opened;
- actionable errors without leaking key material.
