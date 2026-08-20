# Roadmap

The roadmap is ordered by reliability and user impact. Completed items remain listed so a release
can be audited against the intended behavior.

## Reliability foundation

- [x] Give scans and rips explicit terminal success, failure, and cancellation events.
- [x] Make active FFprobe processes cancellable instead of waiting for `Command::output`.
- [x] Cancel active work before allowing the application window to close.
- [x] Preflight every selected title, stream, destination, and filename collision before FFmpeg
  writes the first title.
- [x] Verify that the output directory can be created and written before starting a job.
- [x] Track child processes in a reusable process supervisor and add escalation from graceful
  termination to forced termination.
- [x] Detect unexpected worker/channel termination and return the UI to an actionable state.
- [x] Add structured per-title job results so a partially completed multi-title job can be resumed.
- [ ] Check available space against selected-size estimates, with an explicit override for unknown
  or estimated sizes.

## Honest, efficient desktop workflow

- [x] Edit each selected title's output filename directly in the title list.
- [x] Honor ask-every-time and custom output-directory preferences.
- [x] Honor eject-after-success without ejecting on cancellation or failure.
- [x] Mark settings that are not connected to engine behavior as unavailable until
  their backing capability ships. This includes retry/buffer controls, protection processing,
  streaming, alternate tools, and encoding profiles.
- [ ] Show a preflight summary with selected title/track count, estimated bytes, output paths,
  conflicts, and overwrite choices.
- [ ] Implement reusable filename templates and validate previews in Settings.
- [ ] Apply changed selection defaults to the open catalog without destructively discarding hidden
  duplicate playlists.
- [ ] Persist window size, pane positions, last source, and last output directory.
- [ ] Add an optional post-completion action to reveal the output folder.

## Scanning and optical hardware

- [x] Report determinate candidate/title scan progress and a final skipped-title summary.
- [ ] Detect drive changes and media insertion/removal through native system notifications.
- [ ] Cache stable probe results by source identity and invalidate them on media changes.
- [ ] Improve raw Blu-ray enumeration without requiring a desktop mount.
- [ ] Add read-error retry strategy, drive speed, and bad-sector reporting.
- [ ] Build a reproducible hardware test matrix across authored DVD-Video, Blu-ray, UHD, folders,
  images, USB drives, and multiple FFmpeg versions.

Optical media should be probed serially by default: parallel reads usually increase seeking and
reduce throughput. Optimization should focus on bounded probes, cancellation, caching, and avoiding
duplicate work.

## Accessibility and localization

- [x] Add keyboard shortcuts and menu accelerator labels for open, refresh, cancel, settings, and
  quit actions.
- [ ] Audit accessible names, descriptions, label relationships, focus order, and status/progress
  announcements with a screen reader.
- [x] Reduce fixed panel minimums and allow both main panes to shrink for narrow windows.
- [ ] Route all user-visible window, component, status, log, and error strings through gettext.
- [ ] Extract and package translation catalogs and add a pseudo-locale layout test.

## Architecture and quality

- [ ] Split the application controller into scan-job, rip-job, drive, settings, and presentation
  coordinators with independently testable state transitions.
- [ ] Split probe parsing from process execution and inject process/filesystem adapters into tests.
- [ ] Add controller lifecycle tests for cancellation, close-during-work, stale events, and channel
  disconnection.
- [x] Add continuous integration for formatting, Clippy with warnings denied, workspace tests, the
  FFmpeg smoke test, and minimum-supported-Rust builds.
- [ ] Add desktop-file, icon, AppStream, and package validation.
- [ ] Add fuzz/property tests for DVD IFO, Blu-ray MPLS, FFprobe JSON, naming, and progress parsers.

## Ripping depth

- [x] Parse Blu-ray MPLS play items and segment maps directly.
- [x] Detect exact duplicate playlists without hiding them.
- [ ] Flag suspicious playlists and explain the reason without hiding them.
- [ ] Improve angle handling, seamless branching, and forced-subtitle detection.
- [ ] Add whole-disc backup as a separate job type.

## Formats and encoding

- [x] Introduce output profiles while retaining stream-copy MKV as the safe default.
- [x] Add optional AV1, H.265, H.264, Opus, AAC, and FLAC encoding through the FFmpeg codec
  provider, with encoder availability checks.
- [x] Add MP4 and WebM muxers where selected streams are compatible.
- [x] Add Audio CD discovery, secure extraction, local CD-TEXT tagging, and audio output defaults.
- [x] Add exact MusicBrainz Audio CD matching, a validated CD-TEXT release-search fallback,
  editable tags, and an offline CD-TEXT fallback.
- [x] Add authored DVD/Blu-ray metadata, exact TheDiscDB disc-layout matching, cached optional
  parallel TVMaze/TMDB/TVDB enrichment, conservative title validation, and visible match provenance.
- [ ] Add optional Audio CD checksum verification.

## Platforms and distribution

- [ ] Add native drive providers for Windows and macOS.
- [ ] Produce Flatpak, AppImage, Debian, and RPM packages.
