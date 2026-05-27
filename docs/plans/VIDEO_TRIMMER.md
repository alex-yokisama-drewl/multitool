# Tool: Video Trimmer

> Ephemeral working doc. Built brief → plan, kept live per commit, deleted when the tool ships.
> Third video tool. Substrate is the ffmpeg sidecar (`multitool_core::ffmpeg`), not the
> PCM-buffer path the Audio Trimmer uses — trimming a video is an ffmpeg `-ss`/`-t` call.

## Summary

Trim a single video file to a `[start, end]` range via an ffmpeg **stream copy** (no
re-encode). Output preserves the source container and lands next to the input as
`{stem}_trimmed.{ext}`. UI mirrors the Audio Trimmer (minus fades): a `<video>` preview
with draggable in/out markers + numeric time inputs.

## Decisions locked with the user (2026-05-27)

- **Engine: stream copy** (`-ss`/`-t` + `-c copy`). Near-instant, lossless, accepts any
  container ffmpeg can demux+remux. **Caveat: cut points snap to the nearest keyframe at or
  before the requested start** (input-seek), so the real start can land up to a GOP early
  (often 1–10 s). This is the documented, accepted tradeoff — frame-accurate trimming is a
  later re-encoding variant if it's ever wanted (note in DECISIONS at ship).
- **Output preserves source format** (audio-trimmer contract). No target-format dropdown.
- **Preview: always-available `<video>` player + a simple draggable in/out timeline.**
  Working blind is unacceptable, so the player must show frames for *any* source. Mechanism:
  play the source directly when the WebView can decode it; otherwise generate a **preview
  proxy** (transcode to a web-friendly mp4 in a temp dir) and play that. The trim itself always
  applies to the **original** file via stream copy — the proxy is preview-only.
  - **Native-first, proxy fallback** (my interpretation of "double conversion round trip"):
    try the source in a hidden `<video>` first; if `error`/no-decode fires, transcode a proxy.
    This guarantees "always available" without re-encoding the common mp4/webm case. Detection
    is runtime (`<video>` events), not extension-based — an mp4 can hold HEVC the WebView
    can't decode, so guessing by extension is wrong.
  - **Scrubber = a plain line** (no filmstrip), two draggable handles (start/end) + a playhead.
    **Dragging a handle seeks the preview player to that timestamp** so the user sees the
    corresponding frame. "Play selection" plays start→end (mirrors the Audio Trimmer's
    play-the-window). A popup thumbnail above the handle is a possible nicety — deferred; the
    main-player seek satisfies the requirement.
  - **Preview/output mismatch caveat:** the proxy (and native source) seek frame-accurately,
    but the output start snaps to a keyframe ≤ the chosen point. So the previewed start frame
    may differ slightly from the actual output start. Documented; consistent with the accepted
    stream-copy tradeoff.

## Inputs

- One video file (single-select picker). Reuse the existing `pickVideoFile()` in
  `src/lib/system.ts` (already used by Audio Extractor): mp4, m4v, mov, mkv, webm, avi, 3gp,
  3g2, ts, mts, m2ts, mxf, flv, ogv, wmv, asf, vob, divx, mpg, mpeg. Filter is advisory;
  ffmpeg sniffs the real container.

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `start_ms` | u64 | 0 | Start of trim window, ms from source start. |
| `end_ms` | u64 | source duration | End of window. Clamped to probed source duration in `convert`. `start_ms >= end_ms` (post-clamp) → `ProcessingFailed`. |

No fades, no codec knobs, no target format — output is a stream copy of the source.

## ffmpeg recipe

```
-y -ss <start_s> -i <input> -t <dur_s> -map 0:v? -map 0:a? -c copy -avoid_negative_ts make_zero <output>
```

- `-ss` **before** `-i`: fast input seek; snaps to keyframe ≤ start (the documented caveat).
- `-t <dur_s>` (= end − start) rather than `-to`: unambiguous after an input-seek; sidesteps
  the `-to` relative/absolute ambiguity across ffmpeg versions.
- `-map 0:v? -map 0:a?`: copy all video + audio streams, optional so a video-only or
  audio-less source doesn't fail. **Subtitle / data / attachment streams are dropped in v1** —
  subtitle copy into mp4/mov is a cross-container footgun; A/V-only keeps the copy robust.
- `-avoid_negative_ts make_zero`: output starts at ts 0 so players don't choke on a
  negative/large initial timestamp after the copy-cut.
- The standard `-progress pipe:1 -nostats -hide_banner -loglevel error` prefix is prepended by
  `ffmpeg::run` — don't pass it.

## Output

- **Location:** same directory as the input.
- **Naming:** `{stem}_trimmed.{ext}`, `ext` = source extension (mirrors Audio Trimmer).
- **Duplicate handling:** via `multitool_core::fs::unique_path` → `{stem}_trimmed (1).{ext}`.
  Per [../ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions).
- On any error (incl. cancel) the partial output is unlinked — a half-written clip is garbage.
  Same rule as the Video Format Converter `convert`.

## UX flow

Dashboard → Video Trimmer → pick file → **picked** view → Trim → **done**.

- **picked view:** `<video>` preview (when playable) with a timeline strip carrying draggable
  start/end handles; numeric Start/End inputs (`MM:SS.mmm`); a "Play selection" button that
  seeks to start, plays, and pauses at end (mirrors the Audio Trimmer's play-the-window). When
  the source isn't WebView-decodable, hide `<video>`, keep the timeline (plain bar, no frames)
  + numeric inputs.
- State machine: `idle → loading (probe + native-decode check) → preparing-proxy (only when
  native fails; cancellable, shows transcode progress) → picked → running → done | error`. The
  error arm preserves the picked file (retry without re-picking), like the Audio Trimmer.
- **Cancellation:** Cancel during the running state aborts the signal → `cancel_job` → ffmpeg
  child killed, partial output removed. Stream copy is fast, so the running state may be
  brief; a progress bar still renders off `out_time_us / trim_duration`.

## Backend commands

Three commands beyond the trim itself:

1. **`probe_video_duration(path) → { duration_ms }`** — wraps
   `multitool_core::ffmpeg::probe_duration_secs`. The picked view needs duration up front to
   place the end marker and clamp inputs; `<video>.duration` is unreliable / unavailable for
   proxied sources, so always probe on the backend.
2. **`prepare_preview_proxy(jobId, path) → { proxy_path }`** — only invoked when native
   playback fails. Transcodes the source to a web-friendly mp4 in a temp dir and grants asset
   scope on the proxy. Cancellable + progress-reporting (it can be slow on a long source).
   Proxy recipe (favor speed + small size over fidelity — it's throwaway preview):
   `-i <src> -c:v libx264 -preset ultrafast -crf 28 -vf scale='min(1280,iw)':-2 -c:a aac
   -movflags +faststart <proxy>`. Full-length (no segment streaming in v1).
3. **`trim_video(jobId, path, opts) → JobResult`** — the stream-copy trim, via
   `run_streaming_job`.

**Proxy lifecycle:** written to the OS temp dir under a per-pick name. Best-effort delete via a
`cleanup_preview_proxy(path)` command on new pick / reset / unmount. OS temp is the backstop if
a cleanup is missed. (A future optimization: when only the *container* is unsupported but the
video codec is web-playable, remux-copy instead of re-encoding — note in BACKLOG, not v1.)

## Edge cases

- `start_ms >= end_ms` after clamping → `ProcessingFailed` (no output written).
- `end_ms` beyond source duration → clamped silently (audio-trimmer parity).
- Source unreadable / not a real video → ffmpeg non-zero exit → `ProcessingFailed`.
- Missing input → `FileNotFound`.
- Keyframe snap means the output's actual start ≤ requested start; out duration may be slightly
  longer than `end − start`. Documented, not an error.
- WebView can't decode source → proxy transcode for preview; trim still applies to the original.
- Proxy transcode cancelled mid-prepare → return to `idle` (or keep the picked path for retry),
  partial proxy unlinked.

## Shared-surface touches (all anticipated, not registry violations)

- `src-tauri/src/asset_scope.rs`: add a `VIDEO_EXTS` slice + chain it into `is_media_path`.
  The module doc already says "New media families add an extension list and a `*_EXTS` slice."
- `src/lib/system.ts`: add `videoAssetUrl(path)` (twin of `audioAssetUrl`); reuse
  `pickVideoFile()` and `allowMediaPreview()` as-is.
- `src/lib/time.ts` (**new shared util**): extract `formatMs`/`parseMs` (+ the `TimeInput`
  component → `src/components/`?) out of the Audio Trimmer so both tools consume one copy.
  Approved to do in this PR. Audio Trimmer updated to import from the shared module; its
  existing tests keep passing against the moved functions.
- `src/tools/registry.ts` + `src/app/Dashboard.test.tsx`: the standard registry entry + tile
  assertion.
- `src-tauri/src/tools/mod.rs`: four new `#[tauri::command]` names in `generate_handler!`
  (`probe_video_duration`, `prepare_preview_proxy`, `cleanup_preview_proxy`, `trim_video`).

Tile: category `video`, color `rose` (converter is `teal` — keep them distinct).

## Acceptance

- [ ] Pick a video → trim a sub-range → `{stem}_trimmed.{ext}` plays and starts ≈ at the
      chosen point (within keyframe tolerance), ends at the chosen point.
- [ ] Output preserves the source container/codecs (copy, no quality loss).
- [ ] `<video>` scrubber works for mp4/webm; numeric fallback engages for mkv/avi.
- [ ] Invalid range rejected with no file written; collision lands `(1)`; cancel removes the
      partial; missing input → FileNotFound.
- [ ] Rust ≥80% on `convert.rs`; wrapper + component Vitest; Playwright happy path.

---

## Commit plan (live — flip to done + SHA + gotcha per commit)

| # | Commit | Status |
| --- | --- | --- |
| 1 | `docs(video-trimmer): brief + plan working doc` | done `b6c4422` |
| 2 | `refactor(time): extract formatMs/parseMs (+TimeInput) to a shared module` | done `9c99268` — `formatMs`/`parseMs` → `src/lib/time.ts`; `TimeInput` → `src/components/TimeInput.tsx`; Audio Trimmer imports both; tests moved to `src/lib/time.test.ts`. Gotcha: each commit records the *previous* commit's SHA here (a commit can't contain its own hash). |
| 3 | `feat(video-trimmer): core trim convert + arg/naming logic + unit tests` | done `40f4e4d` — `convert.rs`: `Opts{start_ms,end_ms}`, `convert()` (probe→clamp→validate→ffmpeg copy→cleanup), `derive_output_path` (`{stem}_trimmed.{ext}`), `build_args`. Gotchas: (1) clippy `doc_markdown` treats a doc line starting with `-`/`+` as a list item — keep flag names mid-line. (2) `vec![]` over init-then-push (clippy `vec_init_then_push`). |
| 4 | `feat(video-trimmer): core job orchestrator + duration probe + tests` | done `50b4e32` — `job.rs` single-file `run_job` (`Progress::Started`/`FileProgress`, `JobResult{output,duration_ms}`); `probe.rs` `probe_duration_ms`. Cov: convert.rs 92%, job.rs 94.6%, probe.rs needed its own synth-clip happy-path test to clear 80%. Gotcha: a stream copy can finish before `ffmpeg::run`'s read loop sees a cancel, so run_job has a deterministic cancel check between `Started` and the spawn; `match &events.borrow()[0]` as the last block stmt needs binding to a `let` (Ref temporary lifetime). |
| 5 | `feat(video-trimmer): core preview-proxy transcode + tests` | done `82b24f3` — `proxy.rs` `generate_proxy(source,dest,..)`: libx264 ultrafast crf28, `scale=min(1280\,iw):-2` (comma escaped or the filtergraph parser splits it), yuv420p, aac, `+faststart`; partial unlinked on error. Real synth-clip transcode test confirms the filter string actually runs + the proxy is probe-able. Shell owns `dest` (temp dir). |
| 6 | `feat(video-trimmer): tauri commands + register + asset-scope video exts` | done `a10fe56` — shell `tools/video_trimmer/mod.rs`: `trim_video` (run_streaming_job), `probe_video_duration` (spawn_blocking), `prepare_preview_proxy` (proxy at `temp/multitool-preview-{job_id}.mp4`, grants asset scope on the proxy *inside* the blocking closure so it's set before `tool:complete`), `cleanup_preview_proxy` (guarded to temp dir + `multitool-preview-` prefix). Registered 4 commands; `VIDEO_EXTS` added to `asset_scope`. No shell tests (Windows CI can't launch the shell test exe). |
| 7 | `feat(video-trimmer): IPC wrappers + system.ts (videoAssetUrl/probe/proxy) + tests` | done `b76313e` — `src/lib/tools/videoTrimmer.ts`: `trimVideo`/`preparePreviewProxy` (runJob) + `probeVideoDuration`/`cleanupPreviewProxy` (plain invoke); `videoAssetUrl` added to `system.ts`; `pickVideoFile` reused as-is. 9 Vitest cases. Gotcha: `noUncheckedIndexedAccess` — destructure `onProgress.mock.calls` into a typed tuple, don't index. |
| 8 | `feat(video-trimmer): frontend component + scrubber/player + register tile + tests` | done (SHA in #9) — `VideoTrimmer.tsx` (state machine `idle→loading→preparing→picked→running→done`, native-first w/ `probePlayable` then proxy fallback), `VideoScrubber.tsx` (plain timeline, drag seeks the player), `src/lib/videoPreview.ts` `probePlayable`, `index.ts`/`types.ts`; registered tile (video, rose) + Dashboard test (Video now 3 tiles). 5 component tests. Gotchas: (1) `react-hooks/refs` forbids writing a ref during render — unmount-cleanup effect reads `proxyRef` directly. (2) drop `expect.any(...)` in assertions (`@typescript-eslint/no-unsafe-assignment`) — extract `mock.calls[0]` via typed tuple. (3) `jsx-a11y/media-has-caption` isn't in the eslint config, so don't add a disable directive for it. jsdom logs "HTMLMediaElement pause() not implemented" — harmless. |
| — | **STOP — manual smoke session (per ADDING_A_TOOL step 8)** | — |
| 9 | `test(video-trimmer): Playwright happy-path e2e` | pending |
| 10 | `chore(video-trimmer): ship — DECISIONS entry, backlog, drop working doc` | pending |

### Resolved

- **Subtitles/data/attachments:** dropped in v1 (A/V copy only). Confirmed.
- **Time helpers:** generalized to a shared module **in this PR** (commit 2), Audio Trimmer
  updated to consume it. Confirmed.
- **Preview:** always-available via native-first + proxy fallback (above). Confirmed direction;
  open sub-call is native-first-vs-always-transcode — going native-first (lower regret).
