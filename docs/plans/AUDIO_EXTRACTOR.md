# Tool: Audio Extractor

## Summary
Extract **every** audio track from a single video file as MP3(s), via the bundled ffmpeg sidecar.

## Inputs
- One video file (single-select picker — not a multi-file batch).
- Accepted extensions match the Video Format Converter's `pickVideoFiles` filter (mp4 / m4v / mov / mkv / webm / avi / 3gp / 3g2 / ts / mts / m2ts / mxf / flv / ogv / wmv / asf / vob / divx / mpg / mpeg). The filter is advisory; ffmpeg sniffs the container at decode time, so a renamed file surfaces as `AppError::ProcessingFailed` from the ffmpeg shim's stderr-tail rather than a panic.

## Options
| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| _none_ | — | — | No user-facing knobs in v1. Output format / bitrate / "which tracks" are all baked into the Rust side — the tool extracts every audio stream automatically. |

## Output
- **Format:** MP3, VBR ~190 kbps. ffmpeg recipe per track: `-vn -map 0:a:<i> -c:a libmp3lame -q:a 2`.
  - `-vn` drops the video stream entirely.
  - `-map 0:a:<i>` picks the i-th audio stream by index (`i = 0` for the first audio track, `1` for the second, …).
  - `libmp3lame` is included in the bundled eugeneware ffmpeg build (GPL — same licensing posture as the Video Format Converter; see [../DECISIONS.md](../DECISIONS.md) → "Video stack").
  - `-q:a 2` is LAME's V2 preset (~190 kbps VBR), broadly considered transparent for music. Picked over CBR for better quality-per-byte and over V0 (~245 kbps) to keep files smaller.
- **Track count discovery:** new helper `multitool_core::ffmpeg::probe_audio_stream_count(source) -> AppResult<u32>` parses `ffmpeg -i <input>` stderr for `Stream #N:M(...): Audio: ...` lines and counts them. Same shape as the existing `probe_duration_secs` — both lean on ffmpeg's stable-for-a-decade-plus stderr metadata format, so we still don't bundle ffprobe ([../DECISIONS.md](../DECISIONS.md) → "Video stack"). Zero tracks → `AppError::ProcessingFailed { detail: "no audio streams" }`.
- **Location:** same directory as the input.
- **Naming:**
  - **Single track:** `{stem}_audio.mp3` (e.g. `holiday.mov` → `holiday_audio.mp3`).
  - **Multiple tracks:** `{stem}_audio_1.mp3`, `{stem}_audio_2.mp3`, … in source-stream order (1-indexed for the user; the Rust side maps `i` back to the 0-indexed ffmpeg stream specifier).
  - The asymmetric "number only when ambiguous" rule keeps the common case clean and only adds noise when the source genuinely has multiple tracks.
- **Duplicate handling:** per-track, each target path runs through `multitool_core::fs::unique_path`, so `{stem}_audio.mp3` collides into `{stem}_audio (1).mp3`, `{stem}_audio_1.mp3` collides into `{stem}_audio_1 (1).mp3`, etc. Per [../ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions) — never overwrites silently.

## UX flow
Dashboard → **Audio Extractor** tile (Video category, `amber` color — visually distinct from the `teal` Video Format Converter) → "Select video file" button → file picker → confirm → progress UI streams per-track `current: { trackIndex, trackTotal, fraction }` (mid-encode 0..=1 sourced from `out_time_us / probed_duration`, same source as the Video Format Converter) → on success: "Open output folder" (points at the first output) + "Extract another" → on cancel: state returns to the picked-file view so the user can retry without re-picking → on per-job error: same picked-file view with the error envelope shown above.

Shape is **1 input → N outputs** (the PDF→Images family, not the Format Converter's N-files batch). Per-track progress text reads `"Track 2 of 3 — concert.mkv"` when multi-track, `"Extracting audio — concert.mkv"` when single-track (no point showing "1 of 1"). State machine: `idle → picked → running → done | error-on-picked`. Cancellation: `cancel_job` token wired through `crate::ipc::run_streaming_job`; between-tracks cancel is checked at the top of the per-track loop iteration, mid-track cancel kills the ffmpeg child via the same path [`convert.rs`](../../src-tauri/multitool-core/src/tools/video_format_converter/convert.rs) uses today. On any cancel the in-flight partial `.mp3` is deleted; already-written tracks from prior iterations stay on disk (mirrors Video Format Converter's between-files behavior).

Escape key navigates back to the dashboard (consistent with the other tools).

## Edge cases
- **Source has no audio streams** → `probe_audio_stream_count` returns `0` → `AppError::ProcessingFailed { detail: "no audio streams" }`. UI shows the error envelope in the picked-file view; user picks a different file.
- **Source has multiple audio tracks** → tool extracts all of them. No track-selector UI in v1; if/when that becomes a real ask, add a checkbox list between picker and Extract.
- **MP3 LAME sample-rate restriction** (`8/11.025/12/16/22.05/24/32/44.1/48 kHz`) — libmp3lame *inside ffmpeg* auto-resamples to the nearest accepted rate when the input is off-grid (e.g. 50 kHz audio). The Audio Format Converter's "reject outside LAME's accepted set" rule does NOT apply here because we're going through ffmpeg, not the bare `mp3lame-encoder` crate.
- **Renamed/garbage file** → ffmpeg errors out with "Invalid data found when processing input" during track probe → `ProcessingFailed`, surfaced in UI.
- **Source missing on disk** → `AppError::FileNotFound` from the `try_exists` check at the top of the orchestrator (same pattern as the Video Format Converter's `convert`).
- **Same-folder collision** with a pre-existing `<stem>_audio.mp3` (or `<stem>_audio_N.mp3`) → `unique_path` resolves to a free name per-track. Source never touched.
- **Cancel mid-track** → in-flight partial cleaned up; already-written prior tracks stay on disk; `AppError::Cancelled` surfaced; state returns to `picked`.
- **Cancel between tracks** → orchestrator sees `cancel.is_cancelled()` at the top of the next iteration before spawning the next ffmpeg; returns `Cancelled`. No additional cleanup needed.
- **Cancel before any track encodes** → no output written; state returns to `picked`.

## Acceptance
- [ ] New tool tile under **Video** category on the dashboard, distinct color from the Video Format Converter.
- [ ] Single-file picker accepting the same video extensions as the Video Format Converter.
- [ ] Selecting a single-track video and clicking "Extract audio" writes `<stem>_audio.mp3` next to the input.
- [ ] Selecting a multi-track video and clicking "Extract audio" writes `<stem>_audio_1.mp3` / `<stem>_audio_2.mp3` / … next to the input.
- [ ] Mid-track progress bar updates from `out_time_us / probed_duration`; progress text labels which track (e.g. "Track 2 of 3") on multi-track sources.
- [ ] Cancel during encode aborts the ffmpeg child, removes the in-flight partial, leaves earlier tracks on disk.
- [ ] Source with no audio stream surfaces an error envelope in the UI ("no audio streams") without crashing.
- [ ] Same-folder collision lands at `<stem>_audio (1).mp3` (or `<stem>_audio_N (1).mp3`); source and prior outputs are never overwritten.
- [ ] Rust unit tests on the pure orchestrator cover: single-track happy path, multi-track happy path, no-audio source, missing input, cancel-before-encode, cancel-between-tracks, cancel-mid-track, collision → `unique_path`. Coverage ≥80% on the tool's pure-logic files.
- [ ] Rust unit tests on `probe_audio_stream_count` cover: 0 / 1 / N tracks, garbage input.
- [ ] Vitest wrapper test covers: invokes `extract_audio`, progress filtered by JobId, listener cleanup on success + error, AbortSignal → `cancel_job`.
- [ ] Vitest component test covers: defaults render, picker call, progress text + bar render (single + multi-track), error envelope renders, Cancel aborts the signal.
- [ ] Playwright happy-path: dashboard → tile → pick → extract → "Open output folder" visible.
- [ ] Pre-PR checklist green: fmt → clippy → `cargo test -p multitool-core --all-targets` → pnpm lint/typecheck/test → `pnpm tauri build --no-bundle` → `pnpm test:e2e`.
- [ ] BACKLOG.md "Extract audio from video" entry removed.
- [ ] This working doc deleted on ship.

## Open / non-questions
- **Why separate ffmpeg calls per track, not one ffmpeg call with N outputs?** ffmpeg's `-map 0:a:0 ... out1.mp3 -map 0:a:1 ... out2.mp3` form decodes the source once and writes N files in a single run — measurably faster on large sources. Rejected for v1 because: (a) reuses the existing single-output `convert` shape unchanged, (b) per-track progress mapping is trivial (one ffmpeg's `out_time_us` per track instead of demuxing a multi-output progress stream), (c) between-track cancellation is just `is_cancelled()` at the top of the loop instead of mid-child surgery, (d) the decode-N-times cost is negligible for an offline tool that runs occasionally. If a "extract 8 tracks from a 4-hour Blu-ray rip" use case ever shows up and the wait actually annoys someone, refactoring to one ffmpeg call with N outputs is a self-contained change inside the pure-logic module.
- **Why not the Rust `mp3lame-encoder` crate instead of routing through ffmpeg?** ffmpeg is already bundled and already decodes every video container we accept; piping uncompressed PCM out of ffmpeg into a Rust encoder would double the moving parts for zero quality gain. The libmp3lame bound by `mp3lame-encoder` and the libmp3lame inside the eugeneware ffmpeg build are the same upstream codec.
- **Why no ffprobe-based "does this video have audio" pre-check?** We don't bundle ffprobe (see [../DECISIONS.md](../DECISIONS.md) → "Video stack" — saved ~50–80 MB / install). `probe_audio_stream_count` parses `ffmpeg -i` stderr instead — same trick `probe_duration_secs` uses, same stable-for-a-decade-plus output format.
- **Track metadata (language / title tags) in filenames** — e.g. `concert_audio_eng.mp3` for the English dub. Nice-to-have, but adds metadata parsing complexity and an "is the tag clean enough to put in a filename" sanitization layer. v1 stays index-based; metadata-aware naming is a follow-up.
- **Category placement.** Video category (input is a video; users looking to extract audio from a video reach for "video tools"). BACKLOG entry lives under Video for the same reason.

## Commit-sized plan

Each row is one implementation commit. After every commit, this table flips `pending → done` with the SHA + a one-line gotcha note (per [../ADDING_A_TOOL.md](../ADDING_A_TOOL.md) "Keep the plan live").

| # | Status | Commit | Scope |
| --- | --- | --- | --- |
| 1 | done — `15961cd` | `docs(audio-extractor): plan` | Working doc (brief + this plan). |
| 2 | done — `7a1c352` | `feat(core): probe_audio_stream_count` | `probe_audio_stream_count(path) -> AppResult<u32>` added to [`multitool_core::ffmpeg`](../../src-tauri/multitool-core/src/ffmpeg.rs) alongside `probe_duration_secs`. Pure parser anchors on two substrings on the same line (`Stream #` prefix + `: Audio:` marker) so it ignores banner prose mentioning "Audio". Pure tests cover 0 / 1 / 3 tracks, hex stream id in `[0xNNNN]` (mpegts), Windows CRLF, empty input. **Gotcha:** initially split a string literal across two lines for the CRLF test — rustfmt rejected it; collapsed onto one line. |
| 3 | done — `a211bca` | `feat(audio-extractor): pure-logic extraction` | `multitool-core/src/tools/audio_extractor/{convert.rs, job.rs, mod.rs}` created + registered in [`multitool-core/src/tools/mod.rs`](../../src-tauri/multitool-core/src/tools/mod.rs). `extract_one_track` owns the recipe + naming + partial cleanup; `run_job` probes count + loops. `JobResult { track_count, outputs, duration_ms }` — no `first_output_path` Option dance since single-file shape always yields ≥1 output on success (UI takes `outputs[0]`). Coverage: convert.rs 94.6%, job.rs 93.5%. **Gotcha:** multi-audio synth_clip uses `.mkv` not `.mp4` — matroska accepts arbitrary stream layouts; mp4 multi-audio is technically supported but quirky in some ffmpeg builds. Worth knowing for any future "extract from real mp4 multi-audio" test fixture. |
| 4 | done — `a6a9245` | `feat(audio-extractor): Tauri command shim` | `#[tauri::command] extract_audio(path) -> Result<JobResult, AppError>` added in `src-tauri/src/tools/audio_extractor/mod.rs`; registered in shell's `tools/mod.rs` + `generate_handler!`. Boring delegation through `run_streaming_job`. Nothing surprising. |
| 5 | done — `f0dfcff` | `feat(audio-extractor): TS picker + IPC wrapper` | `pickVideoFile()` added next to `pickVideoFiles` in `system.ts`. `src/lib/tools/audioExtractor.ts` + 7-test Vitest suite. **Gotcha:** initial `import type { AppErrorEnvelope }` tripped both tsc's `noUnusedLocals` and eslint — already re-exported via `export type { AppErrorEnvelope } from "../errors"`, so the direct import was redundant. Drop the import line; keep only the re-export. |
| 6 | pending | `feat(audio-extractor): React UI + register` | Create `src/tools/audio-extractor/{index.ts, AudioExtractor.tsx, types.ts, AudioExtractor.test.tsx}`. State machine: `idle → picked → running → done | error-on-picked`. Progress UI labels track index when total > 1, no label when total == 1. Add tile registration in [`src/tools/registry.ts`](../../src/tools/registry.ts) (`category: "video"`, `color: "amber"`) and extend [`src/app/Dashboard.test.tsx`](../../src/app/Dashboard.test.tsx) to assert the new tile. |
| 7 | pending | `test(audio-extractor): Playwright happy-path e2e` | One smoke flow under `tests/e2e/`: dashboard → Audio Extractor tile → pick → extract → "Open output folder" visible. Add a typed mock in `tests/e2e/mocks/audioExtractor.ts` exposing the same wrapper signature, wire into the alias map in [`vite.config.ts`](../../vite.config.ts). |
| 8 | pending | `docs(audio-extractor): cleanup` | Remove "Extract audio from video" from [BACKLOG.md](BACKLOG.md). Add a [../DECISIONS.md](../DECISIONS.md) entry **only if** something surprising emerged (track-count probe quirks, multi-track ffmpeg gotchas, partial-cleanup behavior worth pinning). Delete this working doc. |

Out-of-table follow-ups expected for each `feat:` / `test:` commit above: a paired `docs(audio-extractor): plan — flip commit #N to done with SHA` commit that updates this table — same cadence as the Video Format Converter's history. Cheap to write; keeps a fresh session able to resume from the doc without rummaging through `git log`.
