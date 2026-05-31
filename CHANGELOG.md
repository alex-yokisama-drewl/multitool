# Changelog

All notable changes to this project follow [Semantic Versioning](https://semver.org/) and are sourced from Conventional Commit history.

## [0.3.0] - 2026-05-31

### Bug fixes

- **pdfium:** Make staged-resource copy in build.rs idempotent (#14)
- **video:** Even-dim scaling, drop _converted suffix, wider picker
- **video-trimmer:** WebM/VP9 preview proxy + minimal player controls
- **video-trimmer:** Play preview from a blob URL, not the asset URL
- **video-trimmer:** Clamp preview to trim window, cap height, sweep proxies
- **video-trimmer:** Drop stream-copy / keyframe-snap copy now that the engine is frame-accurate
- **video-trimmer:** Drop AAC default 192k → 128k (smoke-fix)
- **image-resize:** Gate Fit/Background on aspect mismatch, not lock state

### Build

- **video:** Bundle ffmpeg as Tauri sidecar resource

### CI

- Drop redundant push-to-master trigger (#16)

### Chores

- **image-crop:** Register tool + dashboard tile
- **image-crop:** Ship — DECISIONS entry, backlog + docs, drop working doc
- **video-trimmer:** Ship — DECISIONS entry, backlog, drop working doc
- **lorem:** Ship
- **video-trimmer:** Ship — DECISIONS entry, backlog, drop working doc
- **image-resize:** Register tool + dashboard tile
- **image-resize:** Ship — DECISIONS entry, backlog, drop working doc
- **video-compress:** Ship — DECISIONS entry, drop working doc
- **image-compress:** Ship — DECISIONS entry, drop working doc
- **icons:** Replace placeholder Tauri icons with branded multitool icon

### Documentation

- Drop MSI caveat — confirmed host-specific (#13)
- Reorganize into docs/ and consolidate per-tool plan flow (#15)
- Record HEIC libheif bundling complexity in BACKLOG
- **audio:** Keep plan doc live, codify the convention in ADDING_A_TOOL
- **audio:** DECISIONS entries + delete working doc + BACKLOG cleanup
- **audio-trim:** Phase 1 brief for the Audio Trimmer
- **audio-trim:** DECISIONS entry + delete working doc + BACKLOG cleanup
- **video:** Plan — flip commit #1 to done with SHA + gotchas
- **video:** Plan — flip commit #2 to done with SHA + gotchas
- **video:** Plan — flip commit #3 to done with SHA + gotchas
- **video:** Plan — flip commit #4 to done with SHA
- **video:** Plan — flip commit #5 to done with SHA
- **video:** Plan — flip commit #5b to done with SHA
- **video:** Plan — flip commit #6 to done with SHA
- **video:** DECISIONS entry + delete working doc + BACKLOG cleanup
- **audio-extractor:** Plan
- **audio-extractor:** Plan — flip commits #1 + #2 to done with SHAs
- **audio-extractor:** Plan — flip commit #3 to done with SHA
- **audio-extractor:** Plan — flip commit #4 to done with SHA
- **audio-extractor:** Plan — flip commit #5 to done with SHA
- **audio-extractor:** Plan — flip commit #6 to done with SHA
- **audio-extractor:** Plan — flip commit #7 to done with SHA
- **audio-extractor:** DECISIONS entry + delete working doc + BACKLOG cleanup
- **adding-a-tool:** Require manual smoke session before writing e2e
- **image-crop:** Brief + plan working doc; flip commit #1 done
- **image-crop:** Plan — flip commit #2 to done with SHA
- **image-crop:** Plan — flip commit #3 to done with SHA
- **image-crop:** Plan — flip commit #4 to done with SHA
- **image-crop:** Plan — flip commit #5 to done with SHA
- **image-crop:** Plan — flip commit #6 to done with SHA
- **image-crop:** Plan — flip commit #7 to done with SHA
- **image-crop:** Plan — flip commit #8 to done with SHA
- **image-crop:** Plan — flip commit #9 to done with SHA
- **image-crop:** Plan — flip commit #11 to done with SHA
- **video-trimmer:** Brief + plan working doc
- **backlog:** Drop audio tools + preview proxy, add lorem generator
- **backlog:** Commit to always-on full-re-encode accurate trim; simplify lorem
- **video-trimmer:** Brief for frame-accurate re-engine
- **image-resize:** Brief + plan working doc
- **image-resize:** Flip commit #1 done
- **image-resize:** Flip commit #2 done
- **image-resize:** Flip commit #3 done
- **image-resize:** Flip commit #4 done
- **image-resize:** Flip commit #5 done
- **image-resize:** Flip commit #6 done
- **image-resize:** Record smoke-pause status + PNG-size backlog entry
- **image-resize:** Close smoke pause; drop redundant BACKLOG entry
- **image-resize:** Flip commit #8 done; fmt e2e files
- **video-compress:** Brief + plan working doc
- **video-compress:** Flip commit #2 done
- **video-compress:** Flip commit #3 done
- **video-compress:** Flip commit #4 done
- **video-compress:** Flip commit #5 done
- **video-compress:** Flip commit #6 done
- **video-compress:** Flip commit #7 done
- **video-compress:** Close smoke pause; flip commit #9 done
- **image-compress:** Brief + plan working doc
- **image-compress:** Flip commit #1 done
- **image-compress:** Flip commit #2 done
- **image-compress:** Flip commit #3 done
- **image-compress:** Flip commit #4 done
- **image-compress:** Flip commit #5 done
- **image-compress:** Flip commits #6a and #6b done
- **image-compress:** Flip commit #7 done

### Features

- **audio:** Scaffold audio_format_converter tool + cargo deps
- **audio:** Symphonia-based decode to interleaved PCM
- **audio:** WAV + FLAC encoders (hound, flacenc, claxon for FLAC decode)
- **audio:** MP3 (LAME) + OGG Vorbis encoders, macOS CI autotools step
- **audio:** Channel mode (downmix/upmix) + MP3 sample-rate validation
- **audio:** Batch orchestrator with skip+continue + cancellation
- **audio:** React component for picker, options form, progress, done
- **audio-trim:** Scaffold audio_trimmer module + frontend tile
- **audio-trim:** Pure trim + fade
- **audio-trim:** Wire encode + single-file orchestrator
- **audio-trim:** React UI — waveform, markers, fades, Preview
- **audio-trim:** UX polish — silent range clamp, Play/Stop icon, end-aligned input
- **video:** Ffmpeg shim — spawn, progress parser, cancel
- **video:** Convert + orchestrator (single + batch)
- **video:** Tauri command + TS IPC wrapper
- **video:** React UI + dashboard tile
- **core:** Probe_audio_stream_count
- **audio-extractor:** Pure-logic extraction
- **audio-extractor:** Tauri command shim
- **audio-extractor:** TS picker + IPC wrapper
- **audio-extractor:** React UI + register
- **system:** Supported_raster_formats IPC command
- **lib:** ImageFormats wrapper with memoized fetch
- **image-crop:** Rust crop transform + multi-frame TIFF guard
- **image-crop:** Job orchestrator + crop_image Tauri command
- **image-crop:** TS IPC wrapper
- **image-crop:** React frame editor + tool component
- **video-trimmer:** Core trim convert + arg/naming logic
- **video-trimmer:** Core job orchestrator + duration probe
- **video-trimmer:** Core preview-proxy transcode
- **video-trimmer:** Tauri commands + register + asset-scope video exts
- **video-trimmer:** IPC wrappers + system.ts videoAssetUrl
- **video-trimmer:** Frontend component + scrubber + register tile
- **lorem:** Scaffold tool + 'text' category
- **lorem:** Paragraph generator + tests
- **lorem:** Copy + regenerate UI
- **lorem:** Esc-to-dashboard parity
- **ffmpeg:** Probe_video_stream_params + probe_audio_stream_params
- **video-trimmer:** Codec-matched frame-accurate re-encode engine
- **image-resize:** Rust convert with fit modes + background fill
- **image-resize:** Rust job orchestrator + tauri command
- **image-resize:** TS IPC wrapper
- **image-resize:** React component with lock + fit + bg controls
- **video-compress:** Rust core — convert + job
- **video-compress:** Tauri shim
- **video-compress:** TS IPC wrapper
- **video-compress:** Tool component + register
- **image-compress:** Add libwebp dep via webp crate
- **image-compress:** Rust core — convert + job
- **image-compress:** Tauri shim
- **image-compress:** TS IPC wrapper
- **image-compress:** Tool component + register tile
- **image-compress:** Shared thumbnails + reactive WebP-redirect banner

### Refactoring

- **audio:** Extract audio_codecs shared module
- **image:** Extract RasterFormat into shared module
- **image-format-converter:** Consume IPC for output formats
- **time:** Extract formatMs/parseMs + TimeInput to shared modules
- **image:** Lift JPEG_REENCODE_QUALITY + tiff_frame_count
- **video:** Hoist preview proxy to multitool_core::video
- **video:** Hoist codec mirror tables to multitool_core::video
- **video:** Hoist preview-proxy Tauri commands to shared module
- **image:** Extract shared ImageThumbnail component

### Tests

- **audio:** Vitest for the audioFormatConverter IPC wrapper
- **audio:** Playwright happy-path e2e
- **audio-trim:** Playwright happy-path e2e
- **video:** Playwright happy-path e2e
- **audio-extractor:** Playwright happy-path e2e
- **image-crop:** Playwright happy-path e2e
- **video-trimmer:** Playwright happy-path e2e
- **lorem:** Playwright happy-path e2e
- **image-resize:** Playwright happy-path e2e
- **video-compress:** Playwright happy-path e2e
- **image-compress:** Playwright happy-path e2e

### Style

- **audio-extractor:** Prettier on new TSX files
## [0.2.1] - 2026-05-23

### Bug fixes

- **pdfium:** Bundle native binary as a Tauri resource (#12)

## [0.2.0] - 2026-05-23

### Bug fixes

- **rust:** Drop mobile crate-types so cargo test passes on Windows
- **rust:** Drop default-members so tauri build resolves the shell crate
- Remove malicious postinstall script from package.json (#7)
- **image-format-converter:** Reveal-folder + single-batch flow + thumbnails + retire planning docs (#9)

### CI

- Add cross-platform pipeline and release workflow
- Temp Windows diagnostic — dump test-exe imports on failure
- Bump checkout, setup-node, pnpm/action-setup from v4 to v6

### Chores

- Scaffold tauri 2 + react + ts template
- Configure frontend toolchain (tailwind, shadcn, zustand, router)
- Configure rust backend (error type, tokio, registry stubs)
- Add lefthook hooks and changelog config
- Add test harnesses (vitest, playwright, coverage)
- Lefthook clippy hook covers the workspace
- Deps
- Phase F generalization audit (three extractions + one principled skip) (#10)

### Documentation

- Add technical specification and working agreement
- Add scaffold plan and reference it from CLAUDE.md
- Mark scaffold plan phases A + B done
- Mark scaffold phase C done; add per-phase workflow rules
- Sync stale command references after the default-members revert
- Record configured branch protection in Phase G (#1)
- Scaffold readme, changelog, commands cheat sheet + doc reorg (#2)
- Retire SCAFFOLD_PLAN.md ahead of v0.1.0-scaffold tag (#3)
- Trim DECISIONS, retire per-tool brief, capture follow-ups (#5)

### Features

- PDF → Images tool (Phase 1 / first tool end-to-end) (#4)
- Add Images → PDF tool with reorderable staging (#6)
- Image Format Converter tool (#8)
- **dashboard:** Square color tiles + category grouping (#11)

### Refactoring

- **rust:** Split pure logic into multitool-core to unblock Windows CI

