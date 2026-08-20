# Media support

| Input | Discovery | Rip output | Notes |
|---|---:|---:|---|
| Ordinary media file | Yes | Yes | Any input that the installed FFmpeg supports |
| Mounted DVD / `VIDEO_TS` folder | Yes | Yes | Enumerates titles from `VIDEO_TS.IFO` |
| Raw DVD device | Yes | Yes | Enumerates the title table through `libdvdread` |
| Mounted Blu-ray / `BDMV` folder | Yes | Yes | Main titles are marked; later exact duplicate playlists identify the first matching playlist and are unselected by default |
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

Before online lookup, MediaRip reads optional DVD text and Blu-ray Disc Library XML, including the
disc name, authored title names, and local artwork. Mounted discs and disc folders also receive a
TheDiscDB-compatible MD5 fingerprint made from sorted file sizes. Mounted folders and unmounted
DVD devices with an ISO9660 bridge directory are supported. An exact TheDiscDB hash match can
name main features, episodes, and extras, but every title mapping must also pass duration/size
validation. TheDiscDB and credential-free TVMaze can each be disabled independently; configured TMDB and TVDB credentials enrich exact identities
with canonical title, year, type, provider IDs, episode data, and poster artwork. The providers are queried concurrently; season-labelled TV searches use a shared conservative probable-title review gate, and matching
results raise identity confidence; conflicting title-only results are retained as warnings rather
than applied. A title-only provider result is applied to a movie item only when its runtime uniquely
identifies one scanned title. Raw Blu-ray devices and DVDs without an
ISO9660 bridge directory cannot currently be fingerprinted, and community database coverage varies
by release and region.

An open video disc can be identified explicitly with one public TVMaze show URL or TheTVDB
series/movie URL. MediaRip validates the provider host, extracts the provider ID or slug, and
rescans with that exact user-selected identity. A TVDB API key is still required for TheTVDB URLs;
TVMaze URLs remain credential-free.

For TV DVDs without an exact disc record, an authored label such as `SHOW_S2_D3` can be combined
with the disc's play-all/title grouping and online season order. TVDB DVD order is preferred when
available; TMDB and TVMaze fill missing per-episode runtimes only where episode names agree. These matches remain visibly marked
as probable and separate from title metadata until the user applies one or all of them. Alternate
authored title groups are retained as separate suggestions instead of being silently discarded.
Split-volume labels such as `SHOW_S2_VOL2_D3` are retained as distinct evidence: MediaRip estimates
the total disc count from the season size and episode runtime, then reports any inferred overall
disc number in the source information panel rather than presenting it as authored fact.
When the configured providers supply individual episode runtimes, MediaRip scores every consecutive season window
against the complete authored title group. The comparison normalizes a small consistent timing
difference (including PAL speed-up) and only replaces the position estimate when the best window is
clearly separated from the runner-up. Rounded or identical runtimes remain explicitly ambiguous.
The review row accepts the first episode number for remapping the complete consecutive proposal without another network request;
accepted shifts are remembered by exact disc-content hash and reapplied when that release is scanned
again. Long episodes up to two hours can participate when provider season data supports their length.

Audio CD tables of contents are read with `cdparanoia`; embedded CD-TEXT can be read locally with
`cd-info` and no CDDB network request, or disabled in Settings. Each selected track is securely
extracted to temporary 16-bit, 44.1 kHz stereo PCM and then written by FFmpeg as FLAC, WAV, Opus,
or AAC/M4A. FLAC is the default for both music and audiobooks. Album, artist, title, and track/total
tags are written when the source metadata is available. When an exact MusicBrainz disc ID is not
registered, MediaRip searches for a release using the CD-TEXT album and artist, then validates its
track count, titles, and durations before applying it. MusicBrainz matches also fetch front cover
artwork from the Cover Art Archive and embed it in FLAC, Opus, and AAC/M4A output. WAV does
not provide a portable embedded-cover format. Temporary PCM files are removed after encoding,
failure, or cancellation. Online checksum verification such as AccurateRip is not implemented yet.

At startup, the Messages panel reports whether `ffprobe`, `ffmpeg`, and `cdparanoia` are available.
An absent required tool also produces a direct error before an operation starts.

## Protected media

No decryption keys or circumvention components are bundled. CSS-protected DVDs may be readable when
the user's legally installed `libdvdcss` is integrated with the system DVD stack. AACS/BD+
availability similarly depends on the user's libbluray/libaacs setup and lawful key configuration.
Failures are surfaced from the underlying open-source stack in the Messages panel.
