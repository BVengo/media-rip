# Media support

| Input | Discovery | MKV output | Notes |
|---|---:|---:|---|
| Ordinary media file | Yes | Yes | Any input that the installed FFmpeg supports |
| Mounted DVD / `VIDEO_TS` folder | Yes | Yes | Enumerates titles from `VIDEO_TS.IFO` |
| Raw DVD device | Yes | Yes | Enumerates the title table through `libdvdread` |
| Mounted Blu-ray / `BDMV` folder | Yes | Yes | Main titles are marked; exact duplicate playlists can be grouped |
| Raw Blu-ray device | Yes | Yes | Main-title, playlist, chapter, and authored-stream metadata comes from `libbluray` |
| ISO image | Limited | Limited | Ordinary FFmpeg probing only in this release |
| Audio CD | No | No | Planned |
| Other output containers | No | No | Engine boundary is ready; MKV is the first target |
| Video/audio encoding | No | No | Current output is lossless stream copy |

## Toolchain

DVD title tables and program-cell sector ranges are read through `libdvdread`; Blu-ray playlist
sizes, chapters, clip IDs, exact clip ranges, main-title identity, and authored stream PIDs are read
through `libbluray`. Title and track metadata is then obtained from FFprobe JSON and restricted to
the streams authored into each playlist. When authored size metadata is unavailable, MediaRip samples 30 seconds of
packets to estimate the title size. MKV files are written by FFmpeg with explicit stream maps,
chapter mapping, Matroska muxing, and stream copy. This is an open-source media stack and contains
no MakeMKV dependency.

At startup, the Messages panel reports whether `ffprobe` and `ffmpeg` are available. An absent tool
also produces a direct error before an operation starts.

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
