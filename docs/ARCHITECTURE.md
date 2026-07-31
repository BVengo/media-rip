# Architecture

MediaRip is arranged around stable media concepts rather than a particular GUI toolkit or output
format.

## Dependency direction

```text
media-rip-native  ──────►  media-rip-engine  ──────►  media-rip-core
        │                                                  ▲
        └──────────────────────────────────────────────────┘
```

`media-rip-core` has no GTK, FFmpeg, operating-system, or optical-drive dependency. This keeps
catalogs, selections, progress, errors, and naming reusable by a future CLI, job daemon, or another
native platform frontend.

## Crates

### `media-rip-core`

- `model/catalog.rs`: source, drive, title, chapter, and track data.
- `model/rip.rs`: an immutable rip request built from the selected catalog state.
- `model/events.rs`: scan/rip events consumed by frontends.
- `ports/mod.rs`: `MediaEngine` and cancellation contracts.
- `util/`: presentation-safe size/duration formatting and output naming.

Only domain types belong here. A format-specific FFmpeg flag or GTK widget is a layering violation.

### `media-rip-engine`

- `platform/`: operating-system drive discovery. Linux currently uses `lsblk` and udev properties.
- `source/`: lightweight source classification plus DVD and Blu-ray title discovery.
- `probe/`: FFprobe JSON conversion into core domain models.
- `process.rs`: supervised child-process lifetime, graceful cancellation, and forced termination.
- `rip/`: profile-aware FFmpeg execution, compatibility checks, progress parsing, cancellation, and
  output finalization.
- `engine.rs`: the small adapter implementing the core `MediaEngine` port.

Format additions should normally add a source module and an input specification, not branch
throughout the UI. Encoding belongs beside `rip/` and is selected through a core output profile.

### `media-rip-native`

- `app/state.rs`: UI-independent session state.
- `app/controller.rs`: translates user gestures and engine events.
- `ui/window.rs`: top-level native window composition.
- `ui/components/`: toolbar, source bar, content tree, Info panel, output sidebar, and messages.
- `ui/styling.rs`: the application-level GTK CSS.

GTK callbacks do not perform media I/O. The controller starts engine work on background threads and
polls a typed event channel on the GTK main loop.

Every scan or rip must emit exactly one terminal event: finished, failed, or cancelled. Frontends
use that invariant to leave their busy state safely. External media processes must be polled with a
`CancellationToken`; blocking `Command::output` calls are not suitable for long-running probes.
Supervised processes receive a graceful termination request first and are forcibly terminated if
they remain alive beyond the bounded grace period.
Each scan and rip worker owns a dedicated event sender. If it exits without a terminal event, the
receiver disconnects and the controller restores the idle UI with a retryable error.

## Scan flow

```text
Open source
   │
   ▼
classify DVD / Blu-ray / file
   │
   ▼
enumerate DVD titles or Blu-ray playlists
   │
   ▼
FFprobe each candidate
   │
   ▼
MediaCatalog ──► title tree + Info panel
```

DVD folders use the title table and program-cell sector ranges in their IFO files; raw DVD devices
read the same metadata through `libdvdread`. Blu-ray folders enumerate `.mpls` playlists, while
`libbluray` supplies playlist byte ranges and raw-device enumeration.

## Rip flow and output safety

Each selected item becomes one FFmpeg process with explicit stream maps and a typed output profile.
The default profile uses `-c copy`; encoding profiles name their codecs and container explicitly.
FFmpeg writes a uniquely named hidden `.part` file beside the destination. A successful process is
renamed to the requested output; cancellation or failure removes only the partial file created by
the current job. Before the first process starts, the complete request is checked for profile/track
compatibility, encoder availability, stale title/stream IDs, duplicate output names, existing
destinations, and output-directory writability. Existing files are rejected unless a future caller
explicitly opts into overwrite.

## Extension rules

- Keep MakeMKV-specific compatibility out of the engine. The engine is an independent media stack.
- Add fields to core models only when they make sense across backends.
- Never block the GTK main thread with scanning, subprocess waits, or filesystem traversal.
- Preserve streams by default; make transcoding an explicit profile.
- Treat decryption as a capability provider with a visible status, never a hidden external binary.
