# Tool: Video Format Converter

> Phase 1 brief. Plan / commit-sized task list goes below this once the brief is approved.

## Summary

Convert video files between common container/codec combinations (mp4, webm, mkv) using a bundled ffmpeg sidecar binary. Mirrors the audio-format-converter UX: pick files → pick target format → convert.

## Backend choice (decided up-front)

**Bundled ffmpeg sidecar.** Per-OS prebuilt static ffmpeg binary fetched in `build.rs` and staged under `src-tauri/resources/ffmpeg/`, declared in `tauri.conf.json` → `bundle.resources`, resolved at runtime via `app.path().resolve("resources/ffmpeg/ffmpeg<.exe>", BaseDirectory::Resource)` and invoked as a child process via `std::process::Command` (or `tokio::process::Command` if we need async stdout/stderr drainage).

Mirrors the pdfium recipe — same `build.rs` download/extract/cache pattern, same `bundle.resources` plumbing. Differences from pdfium:

- It's an executable, not a library — no FFI, no `Pdfium::bind_to_library`. Just `Command::new(path).args(...).spawn()`.
- Source: official static builds. Candidates (need to pick + pin in plan phase): [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (Linux + Windows, GPL/LGPL flavors, well-tagged), [eugeneware/ffmpeg-static](https://github.com/eugeneware/ffmpeg-static) (npm distribution of evermeet/gyan/johnvansickle builds), or pull per-OS from each canonical mirror (evermeet.cx for macOS, gyan.dev for Windows, johnvansickle.com for Linux). Decision deferred to plan phase.
- Bundle size hit: ~30–80 MB per OS-arch depending on which build flavor. Acceptable for a learning project; will land a [DECISIONS.md](../DECISIONS.md) entry on the trade-off.
- License surface: ffmpeg static builds are typically GPL (because of x264 + libx265 + libfdk-aac inclusion). LGPL-only builds exist but drop H.264 encoding, which kills the v1 mp4 output. We accept GPL for this learning project; documented in DECISIONS.

A thin `multitool-core::ffmpeg` shim (mirroring `multitool-core::pdfium`) will hold the resolved binary path + a `run(args, on_progress, cancel) -> AppResult<()>` helper that:

- Spawns ffmpeg with `-progress pipe:1 -nostats -hide_banner -loglevel error`.
- Reads `-progress` key=value lines from stdout, parses `out_time_us` against the probed input duration, emits a 0–1 progress float to the supplied callback.
- Captures stderr into a ring buffer so a non-zero exit can include the tail in `AppError::ProcessingFailed { detail }`.
- Honours cancellation by `child.kill()` and reaping; the spawned task drops the registry token on exit.

The shim is the only place that knows ffmpeg's CLI shape — per-tool code (this tool, plus future Video Compress / Trim / Extract Audio) calls `run(opts.into_args(), …)`.

## Inputs

Multi-file picker, video files only. Picker extension filter:

`mp4`, `m4v`, `mov`, `mkv`, `webm`, `avi`, `3gp`, `3g2`, `ts`, `mts`, `m2ts`, `mxf`, `flv`, `ogv`, `wmv`, `asf`, `vob`, `divx`, `mpg`, `mpeg`

Sniffing: file-extension based at the picker (consistent with audio-format-converter); ffmpeg itself does format auto-detection, so a misnamed extension still works at runtime — the picker filter is purely a UX guard. Empty selection = no-op. Picker lives in [`src/lib/system.ts`](../../src/lib/system.ts) as a new `pickVideoFiles()` helper.

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| Target format | enum `Mp4 \| Webm \| Mkv` | `Mp4` | Single dropdown. Codec choices baked in per format (table below). |

**Baked-in codec recipes** (no UI for these in v1 — they're the whole point of "format dropdown only"):

| Target | Container | Video codec | Video quality / filters | Audio codec | Audio quality |
| --- | --- | --- | --- | --- | --- |
| `Mp4` | mp4 | H.264 (libx264) | CRF 23, `-preset medium`, `-vf scale=trunc(iw/2)*2:trunc(ih/2)*2`, `-pix_fmt yuv420p` | AAC (native ffmpeg) | 128 kbit/s |
| `Webm` | webm | VP9 (libvpx-vp9) | CRF 32, `-b:v 0`, `-row-mt 1`, `-vf scale=trunc(iw/2)*2:trunc(ih/2)*2` | Opus (libopus) | 96 kbit/s |
| `Mkv` | matroska | copy (`-c:v copy`) | n/a (stream copy) | copy (`-c:a copy`) | n/a |

Rationale for the picks:

- **mp4 = H.264 + AAC** is the universal-compatibility default; CRF 23 is x264's documented "visually lossless-ish" sweet spot. The scale filter rounds odd source dimensions down to even (H.264 in 4:2:0 fails the encode on odd dims with "height not divisible by 2"). `-pix_fmt yuv420p` forces 4:2:0 chroma so the output plays everywhere; without it an input in yuv444p or 10-bit produces an mp4 some players refuse.
- **webm = VP9 + Opus** is the patent-free pairing modern browsers expect; CRF 32 with `-b:v 0` is the constant-quality mode VP9 actually wants (`-b:v 0` disables the bitrate cap that VP9's defaults otherwise impose). Same even-dimension scale filter — libvpx-vp9 also rejects odd dimensions on most configurations.
- **mkv = stream copy** is near-instant remux; if user wants real re-encoding into mkv, that's the v2/Video Compress story. No scale filter (nothing being re-encoded).

If the user picks mkv and the source streams are codecs mkv doesn't accept (vanishingly rare in practice — mkv is permissive), ffmpeg errors and we report the per-file failure. We will not silently re-encode to "fix" a copy.

## Output

- **Location:** same directory as the input (per [ARCHITECTURE §3.3](../ARCHITECTURE.md#33-file-io-conventions)).
- **Naming:** `{stem}.{ext}` (no per-tool suffix — matches the audio + image format-converter precedent's clean naming where the extension change is the only differentiator).
- **Duplicate handling:** [`multitool_core::fs::unique_path`](../../src-tauri/multitool-core/src/fs.rs) — appends ` (1)`, ` (2)`, … per spec. Never overwrite.
- **Same-format conversion** (e.g. user picks mp4 for an mp4 input): allowed. Output collides with the source path, so `unique_path` resolves it to `{stem} (1).{ext}` — same disambiguation policy as the other tools.

## UX flow

Dashboard → Video Format Converter tile → state machine:

1. **idle** — empty drop area + "Pick files" button; target-format dropdown defaulting to `Mp4`.
2. **picked** — file list rendered (filename + size); Convert button enabled. User can change format or re-pick.
3. **running** — per-file progress bar (0–100% from ffmpeg `-progress`), file-N-of-M counter, Cancel button. Webview disables format change and re-pick during run (cancel ends the run, returns to **picked** with whatever was already written intact).
4. **done** — summary: N converted, K skipped (with per-file error messages collapsed under a "Show details"), "Reveal in folder" opens the output directory of the first successful file.
5. **error** — orchestrator-level error (e.g. ffmpeg binary missing, output dir not writable). Picked files preserved so user can retry.

Cancellation: `cancel_job` kills the in-flight ffmpeg subprocess. Files already written stay on disk (per the audio-format-converter precedent; we don't try to roll back partial output). The in-flight file's partial output is deleted before the job exits — partial video files are useless and confusing.

## Edge cases

- **ffmpeg binary missing at runtime** (resource path didn't resolve, e.g. dev build with stale resources/). Orchestrator returns `AppError::ProcessingFailed { detail: "ffmpeg binary not found at …" }` before processing any file. Distinct from per-file failures.
- **Source file unreadable / not actually a video.** ffmpeg exits non-zero; we record per-file `Skipped { path, reason }` and continue. Same shape as audio-format-converter's skip path.
- **Output path collides with input** (target format == source format): `unique_path` resolves the collision to `{stem} (1).{ext}`; source is never overwritten. Verified in tests.
- **Cancellation arrives between files**: the next file simply isn't started; `child.kill()` is a no-op.
- **Cancellation arrives mid-encode**: `child.kill()` reaps the ffmpeg process; partial output file is unlinked; orchestrator returns `Cancelled` after the in-flight file's cleanup.
- **Zero-byte / zero-duration source** (some screen recorders produce these): ffmpeg's `-progress` never reports `out_time_us`; progress bar would sit at 0 then jump to 100. Acceptable; not worth a workaround.
- **Source with no audio stream** (a silent screen capture): mp4/webm targets are fine — ffmpeg just doesn't add an audio track. mkv copy mode is fine too.
- **HDR / 10-bit sources** (mostly `.mkv` HEVC): re-encoding into x264 8-bit will tonemap or banding-clip. v1 accepts this; will document in DECISIONS so a future "preserve HDR" issue can reference it.
- **Permission denied on output** (read-only directory): per-file `Skipped` with the permission error from ffmpeg.
- **Very long files** (multi-hour). Progress callback throttling: emit at most every ~250ms to avoid IPC chatter (audio precedent).

## Acceptance

- [ ] Tool tile appears on dashboard under the `video` category. (New category — extends `ToolCategory` union + `toolCategories` list in [`src/tools/registry.ts`](../../src/tools/registry.ts). Acknowledged as the one deliberate shared edit per [ADDING_A_TOOL §5](../ADDING_A_TOOL.md). No new tile-color token — tile uses the existing `teal` token, currently unused by any tool.)
- [ ] Picking 1+ video files, choosing a target format, and clicking Convert produces output files in the same directory with the target extension, name-collision safe via `unique_path`.
- [ ] Each format (mp4 / webm / mkv) round-trips at least one fixture file end-to-end in a Rust integration test (small synthetic clips, generated by ffmpeg itself in a build-time test fixture or committed as binary fixtures).
- [ ] Cancellation mid-encode kills the ffmpeg child, deletes the partial in-flight output, and returns `Cancelled`.
- [ ] Per-file failure (corrupt source) is reported as a skip; the batch continues.
- [ ] Pre-PR gates pass on all three CI OSes: fmt / clippy / `cargo test -p multitool-core --all-targets` / pnpm lint / typecheck / vitest / `pnpm tauri build --no-bundle` / Playwright happy path.
- [ ] `multitool-core/build.rs` and `src-tauri/build.rs` both download + cache + stage ffmpeg, mirroring the pdfium pattern; both pinned to the same `FFMPEG_TAG` constant; both have the "bump together" comment.
- [ ] DECISIONS.md gains entries for: ffmpeg sidecar choice (vs. ffmpeg-next bindings), GPL licensing acceptance for this learning-project build, and the codec-recipe defaults.

---

## Resolved decisions (carry into the plan)

- **ffmpeg source:** [eugeneware/ffmpeg-static](https://github.com/eugeneware/ffmpeg-static) at tag `b6.1.1` (ffmpeg 6.1.1). Single source covers all five target platforms (linux x64/arm64, darwin x64/arm64, win x64). Bare-binary downloads — no archive extraction. `FFMPEG_TAG` pin lives next to `PDFIUM_TAG` in both build scripts with the same "bump together" comment.
  - **Why not BtbN + evermeet** (originally chosen): research showed evermeet doesn't ship macOS arm64 (forcing a third source) and BtbN's static binaries are ~160 MB each — twice the size of eugeneware's leaner builds. Eugeneware: one source, half the bundle, covers darwin-arm64 natively. Trade-off accepted: trusts one re-distributor maintainer, slightly older ffmpeg pin.
  - **Why not bundle ffprobe**: ffprobe adds another full ffmpeg-sized binary (~50–80 MB/install). For duration probing (the only thing we need from ffprobe), parsing `ffmpeg -i <file>`'s stderr `Duration: HH:MM:SS.cc` line is the standard pattern in every ffmpeg-wrapper project — the line format has been stable for 10+ years. Half the install footprint, one binary per OS.
- **Bundle size reality:** ~43 MB (darwin-arm64) to ~79 MB (win32-x64) per OS-arch added to installed bundle. Compared to pdfium's ~5 MB this is heavy but acceptable for a learning project. Documented in DECISIONS in commit #7.
- **Test fixtures:** synthesize 1-sec clips at test time via the bundled ffmpeg (`-f lavfi -i testsrc=duration=1 -f lavfi -i sine=duration=1 -shortest`) into `TempDir`. No binary fixtures committed. Tests fail loudly when the binary isn't on disk — that's the right signal.
- **Tile color:** `teal` (existing token, currently unused). No CSS or registry-union edit for color.
- **Category:** new `video` category in `ToolCategory` union + `toolCategories` list. Tile lands at the end of the `video` group on the dashboard (only video tile in v1, so position is moot).

---

## Plan — commit-sized tasks

Working pattern lifted from the audio-trimmer commit log (one feature axis per commit, plan-row flips after each push per [feedback_update_working_doc_per_commit](../../CLAUDE.md)). Status legend: `pending` / `in-progress` / `done`. Each `done` row gets the SHA + a one-line gotcha note when it lands.

| # | Status | Commit | What lands |
| --- | --- | --- | --- |
| 1 | done `601b48c` | `build(video): bundle ffmpeg as Tauri sidecar resource` | `FFMPEG_TAG = "b6.1.1"` pin in `multitool-core/build.rs` + `src-tauri/build.rs`. Per-OS bare-binary download from eugeneware/ffmpeg-static into `OUT_DIR` (multitool-core) + staged copy under `resources/ffmpeg/` (src-tauri). `needs_copy` guard mirrors the pdfium dev-watcher fix. `tauri.conf.json` → `bundle.resources` extended to `resources/ffmpeg/*`; `.gitignore` extended to exclude staging dir. `FFMPEG_BIN_PATH` env override mirrors `PDFIUM_LIB_PATH` for offline / CI cache. No archive extraction needed — eugeneware ships bare binaries. **Gotchas:** (a) eugeneware's `b6.1.1` tag actually contains ffmpeg 7.0.2 binaries on Linux (re-distributed from johnvansickle), not 6.1.1 — fine, codec set we need is all present (libx264/x265/vpx/opus/lame/vorbis). (b) `fs::copy` preserves Unix mode bits so `make_executable` after staging is defensive-only — kept for the `FFMPEG_BIN_PATH` override path where source may be 0644. (c) All five Rust/JS gates pass locally including `pnpm tauri build --no-bundle` (release compile, 37s). Linux x64 host only; cross-OS download paths unverified until CI runs. |
| 2 | done `1017d2c` | `feat(video): ffmpeg shim — spawn, progress parser, cancel` | New [`multitool-core/src/ffmpeg.rs`](../../src-tauri/multitool-core/src/ffmpeg.rs) mirroring `pdfium.rs`. `init(path)` registers the resolved binary path; `run(args, on_progress, cancel)` spawns the child with the `-progress pipe:1 -nostats -hide_banner -loglevel error` prefix, drains stdout line-by-line for `out_time_us`, throttles callbacks to 250ms, ring-buffers the last 64 stderr lines for failure detail, kills + reaps on cancel; `probe_duration_secs(path)` parses the `Duration:` line from `ffmpeg -i`'s stderr. 8 unit tests on the parsers + 3 smoke tests (synth-clip encode, duration probe, mid-encode cancel — all pass in ~0.5s). Shell-side `init` wired from `src-tauri/src/lib.rs::run` next to the pdfium init. **Gotchas:** (a) `run`'s args parameter is `IntoIterator<Item: AsRef<OsStr>>` — same shape as `Command::args` — so callers can pass `&[&str]` today and `Vec<OsString>` later without changing the signature. (b) `suppress_console_window` `#[cfg(windows)]` is needed: without `CREATE_NO_WINDOW = 0x0800_0000` the Tauri GUI would briefly flash a cmd.exe when ffmpeg spawns. (c) stderr ring buffer is drained on a dedicated thread because ffmpeg can exceed the OS pipe buffer (~64KB on Linux) and a full stderr pipe would deadlock the child. |
| 3 | done `c8d4397` | `feat(video): convert + orchestrator (single + batch)` | `multitool-core/src/tools/video_format_converter/{mod.rs,convert.rs,job.rs}`. `TargetFormat::{Mp4,Webm,Mkv}` + recipe table → `Vec<OsString>` arg builder (pure, fully unit-tested). `convert(source, opts, on_file_progress, cancel)` probes duration via `ffmpeg::probe_duration_secs`, calls `ffmpeg::run` with built args, emits per-callback fraction `out_time_us/duration` clamped to `[0,1]`, deletes partial output on any error (incl. cancel). `job.rs` orchestrates: `Progress::{Started,FileProgress,Succeeded,Skipped}`, empty-inputs → `ProcessingFailed`, between-file + mid-encode cancel both surface as `AppError::Cancelled`, per-file failures → skip-and-continue. Integration tests synthesize 1–30s clips via the bundled ffmpeg and round-trip through mp4/webm/mkv. **18 new unit tests (197 total in multitool-core lib), all 3 smoke + e2e integration suites green.** **Gotchas:** (a) Two callbacks need shared access to the caller's `on_progress` emitter — the orchestrator wraps it in `RefCell<F>` so the inner per-file FileProgress callback can re-enter the same emitter. Errors from FileProgress emission are captured in a sibling `RefCell<Option<AppError>>` and propagated after `convert` returns, because `ffmpeg::run`'s callback signature is infallible. First implementation buffered fractions and flushed after `convert` returned, which broke the mid-encode-cancel test (the test triggers cancel from inside the FileProgress handler, but deferred handlers fire too late). Synchronous emission via RefCell fixes it. (b) Clippy `cloned_ref_to_slice_refs` insists on `std::slice::from_ref(&input)` over `&[input.clone()]` — the audio orchestrator already follows that pattern; this commit matches. |
| 4 | done `d40f98d` | `feat(video): Tauri command + TS IPC wrapper` | `src-tauri/src/tools/video_format_converter/mod.rs` — `#[tauri::command] convert_video_format` using `crate::ipc::run_streaming_job` (12 lines, copy of the audio shim). Registered in `register_commands`. `pickVideoFiles()` in `src/lib/system.ts` with the mp4/mov/mkv/webm/avi filter. `src/lib/tools/videoFormatConverter.ts` mirrors the audio wrapper — Progress union with `file-progress` variant for mid-encode fractions. 7 Vitest tests on the wrapper (103 frontend tests total). No new gotchas — all the shape work is the same boilerplate as the other converters. |
| 5 | done `c68475a` | `feat(video): React UI + dashboard tile` | `src/tools/video-format-converter/{index.ts,VideoFormatConverter.tsx,types.ts}` with `idle → staging → running → done | error` state machine (mirrors `AudioFormatConverter`). Target-format radios for MP4/WebM/Matroska, per-file `<Progress>` bar bound to `state.current.fraction`, Cancel button, error envelope + skip-summary details. `ToolCategory` union + `toolCategories` list extended for new `"video"` category in `registry.ts`. Dashboard test updated to assert the new tile + section ordering + `teal` color. 7 component Vitest tests (110 total frontend, +7). **Gotchas:** (a) The radix `Progress` primitive in this codebase doesn't expose `aria-valuenow` reliably in the JSDOM test env (radix's source sets it but JSDOM/RTL didn't pick it up). Switched the progress-bar assertion to `aria-label` (which the component sets explicitly with the percentage) — same `state.current.fraction` value, much more stable across radix version bumps. (b) Test mock for `convertVideoFormat` needs the real `ConvertHooks` type imported from the lib, not `any` — `@typescript-eslint/no-unsafe-call` catches the unsafe `.onProgress(...)` call otherwise. |
| 5b | done `2ec6d5f` | `fix(video): even-dim scaling, drop _converted suffix, wider picker filter` | Emerged from manual testing. (a) Real `ProcessingFailed` on `webm → mp4` of a 1062×1043 source — libx264 requires even dimensions in 4:2:0. Added `-vf scale=trunc(iw/2)*2:trunc(ih/2)*2 -pix_fmt yuv420p` to mp4 recipe, scale filter only to webm. (b) Dropped the `_converted` suffix per user request — output is now `{stem}.{ext}`, same-format collisions route through `unique_path` to `{stem} (1).{ext}`. (c) Widened the picker filter from 5 to 20 common video extensions (m4v/3gp/ts/mts/m2ts/mxf/flv/ogv/wmv/asf/vob/divx/mpg/mpeg added). (d) Skipped video thumbnails for v1; added BACKLOG entry with both `<video>` and ffmpeg-extract paths sketched. Convert.rs + job.rs tests updated for the new naming. |
| 6 | done `c27b34c` | `test(video): Playwright happy-path e2e` | `tests/e2e/mocks/videoFormatConverter.ts` mirrors the real wrapper, streams Started → FileProgress → Succeeded per file at ~20ms intervals so the running view actually renders the progress bar. `tests/e2e/mocks/system.ts` extended with `pickVideoFiles()` returning two paths. `vite.config.ts` alias map extended for the new wrapper. `tests/e2e/video-format-converter.spec.ts` covers: dashboard → tile → staging (2 files) → toggle target radio (mp4 → webm) → Convert → done state with 2 converted + Open output folder + Convert another buttons visible. Failure paths stay at the Vitest unit level. **Full e2e suite still green:** 6 passed + 1 pre-existing skip in ~3s. |
| 7 | pending | `docs(video): DECISIONS entries + delete working doc + BACKLOG cleanup` | New DECISIONS entries: "Video: bundled ffmpeg sidecar over ffmpeg-next bindings" (license + CI surface rationale), "Video: GPL ffmpeg accepted for learning-project build" (vs LGPL-only at the cost of H.264 encode), "Video: format-dropdown-only v1 with baked codec recipes". Delete `docs/plans/VIDEO_FORMAT_CONVERTER.md`. Remove "Video format conversion" from `docs/plans/BACKLOG.md`. |

### Pre-PR gates (same as the checklist in [../CLAUDE.md](../../CLAUDE.md))

1. `cd src-tauri && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p multitool-core --all-targets`
2. `pnpm lint && pnpm test && pnpm typecheck`
3. `pnpm tauri build --no-bundle` (sanity-checks the bundled-resource wiring + that the Tauri shell compiles `--release`)
4. `pnpm test:e2e`
5. CI green across linux / macos / windows before self-merge. **High native-deps risk** — every commit that touches `build.rs` or the sidecar shim deserves a CI sweep, not just a local pass. (Push to a `feat/video-format-converter` branch; per [project_ci_triggers](../../CLAUDE.md), CI fires on PR open, not on bare feature-branch pushes — so open the PR early in draft state if I want CI feedback before commit #7.)

### Known gotchas to remember mid-build (will move to inline notes as they bite)

- **Duration probing via `ffmpeg -i` stderr.** No `ffprobe` bundled (saves ~50–80 MB/install). The standard pattern: invoke `ffmpeg -i <file>` with no output spec, ffmpeg writes `Input #0, ...` then `  Duration: 00:01:23.45, start: ...` to stderr, then exits non-zero with "At least one output file must be specified". Parse the `Duration:` line with a `\s*Duration: (\d+):(\d+):(\d+)\.(\d+)` regex. Stable since at least ffmpeg 0.5; every wrapper project does this. Live-on probe goes in commit #2's `ffmpeg::probe_duration_secs(path)`.
- **Windows arm64.** eugeneware ships only `ffmpeg-win32-x64`, no winarm64. We already gate pdfium on x64-only for Windows — same gating applies. No new platform exclusion to document.
- **Asset naming on Windows.** eugeneware's `ffmpeg-win32-x64` asset is a bare PE binary with no `.exe` extension. Windows will refuse to spawn it without one — rename to `ffmpeg.exe` at extract/stage time. The other platforms keep `ffmpeg` (no extension).
- **Binary perms on Unix.** eugeneware's GitHub release downloads come over HTTPS with mode `0644`. After download, `chmod +x` (or `set_permissions` to `0o755`) before exposing the path. macOS in particular needs the executable bit set; chmod 0644 binaries just fail to spawn with EACCES.
- **macOS Gatekeeper on the bundled binary.** In dev on macOS, the bundled ffmpeg binary may be quarantined and require manual `xattr -d com.apple.quarantine`. The bytes come from a HTTPS download (no quarantine flag set by `curl`/`ureq`) so this should *not* hit us at build time, only at install time on signed bundles — and we don't sign macOS per project policy. Document in DECISIONS only if seen.
- **`ffmpeg -progress pipe:1` on Windows.** The line endings are `\r\n`; the parser should split on `\n` and trim `\r`. Easy to forget on Linux.
- **First-run perf on `pnpm tauri dev`.** First build downloads pdfium + ffmpeg (ffmpeg adds ~30 MB compressed download, ~76 MB on disk for linux-x64). Worth a note in DEV README only if it becomes a complaint.
- **Resource path resolution in dev vs bundled.** `app.path().resolve("resources/ffmpeg/ffmpeg<.exe>", BaseDirectory::Resource)` resolves to `src-tauri/resources/ffmpeg/` in dev and the bundled resource path in installed builds. Both have to work. pdfium already proves the pattern — copy it verbatim.
- **eugeneware GitHub release tag.** Tag is `b6.1.1` (`b`-prefixed = "binaries for ffmpeg X.Y.Z"). Asset URLs:
  `https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-<os>-<arch>` (bare binary)
  or `.gz` for compressed (~3x smaller download). Use the bare form to keep build.rs simple — no gzip decode step, just download + chmod.
