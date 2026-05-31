# Tool: Video Compress

## Summary

Re-encode a video at a user-selected quality (single slider), keeping the source's container and codec family, to produce a smaller file. Audio is stream-copied bit-perfect in v1.

## Inputs

- One video file via `pickVideoFile()` (same picker / extension allowlist the Video Trimmer uses).

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `quality` | `f64` in `[0.0, 1.0]` | `0.5` | `0.0` = smallest file, `1.0` = highest quality. The slider value is the wire shape; the Rust side maps it to ffmpeg's CRF per source codec (table below). |

No audio knob, no codec/container picker, no bitrate field. Container + video codec are preserved by extension; audio is `-c:a copy`. See [BACKLOG.md](BACKLOG.md) for the "scale audio bitrate with the slider" follow-up that's deliberately deferred.

## Output

- Location: same directory as the input.
- Naming: `{stem}_compressed.{ext}`.
- Duplicate handling: per [../ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions) — `unique_path` resolves collisions to `{stem}_compressed (1).{ext}`.

## UX flow

1. Dashboard → Compress tile → empty state with a "Select video file" button.
2. User picks a file → "Loading…" while [`probeVideoDuration`](../../src/lib/tools/videoTrimmer.ts) runs, then "Preparing preview" with a progress bar while the proxy transcodes (reuses the trimmer's WebM proxy pipeline verbatim — see "Shared `multitool_core::video`" below).
3. Picked state: filename + duration up top; the same minimal player the trimmer ships (play/pause + seek + volume, no native `controls`, no trim scrubber); below it a single horizontal slider labeled **"Smaller file"** on the left and **"Higher quality"** on the right. No number, no codec name, no CRF readout. Buttons: "Compress", "Pick different file".
4. Compress → "Compressing" view with a progress bar; "Cancel" button kills the ffmpeg child + unlinks the partial output.
5. Done → filename, "Open output folder", "Compress another".

Error states route the user back to the picked view (file + slider position preserved) with the `AppError` banner above the slider, same shape as the trimmer's `picked.error` slot.

Cancellation: token-checked before the encode starts; mid-encode goes through `ffmpeg::run`'s child-kill + partial-output unlink. Cancelling during the preview-proxy transcode returns to the idle state silently (same as the trimmer).

## Slider → CRF mapping

Linear interpolation between two per-codec anchors. `slider=1.0` matches the trimmer's "perceptually transparent at one re-encode" CRF so the two tools' "best" position stays consistent.

| Source codec | `slider=0` (smallest) | `slider=1` (best) | Encoder |
| --- | --- | --- | --- |
| H.264 | CRF 32 | CRF 18 | `libx264 -preset medium` |
| HEVC | CRF 34 | CRF 20 | `libx265 -preset medium` |
| VP9 | CRF 40 | CRF 28 | `libvpx-vp9 -b:v 0 -row-mt 1` |
| AV1 | CRF 42 | CRF 30 | `libaom-av1 -cpu-used 6 -row-mt 1` |
| VP8 | CRF 25 (fixed `-b:v 1M`) | CRF 10 (fixed `-b:v 1M`) | `libvpx` |
| Fallback (unknown source codec) | CRF 32 | CRF 18 | `libx264 -preset medium` |

Values are tunable during smoke. `-pix_fmt` is mirrored from the probed source (no silent 10-bit → 8-bit downgrade), same shape as the trimmer. The shared `multitool_core::video::reencode` module owns the table — both tools read from it (trimmer pins `slider=1.0`).

VP8 keeps a fixed `-b:v 1M` cap in v1 to preserve the trimmer's existing behavior at its transparent CRF; scaling the VP8 bitrate cap with the slider would change two things at once on a codec that's rare in real-world input.

## Shared `multitool_core::video` (refactor lands before tool work)

The user asked us to generalize the reusable bits from the trimmer; doing it as a pre-tool refactor keeps the diff per commit small. **Hoist:**

- `multitool_core/tools/video_trimmer/proxy.rs` → `multitool_core/video/proxy.rs`. Public API (`generate_proxy`) unchanged.
- `select_video_encoder_flags` + `select_audio_encoder_flags` from `video_trimmer/convert.rs` → `multitool_core/video/reencode.rs`. The fixed-CRF trimmer flags become "trimmer presets" the trimmer keeps calling; the compress tool adds a CRF-parameterised builder that takes `(codec, crf)` / `(codec, bitrate)` and returns a flag vec.
- Shell: `prepare_preview_proxy` / `cleanup_preview_proxy` / `cleanup_stale_proxies` + the `PROXY_PREFIX` guard from `src-tauri/src/tools/video_trimmer/mod.rs` → `src-tauri/src/preview_proxy.rs` (parallel to `asset_scope.rs`). Registered once; both tools' frontends import the same wrappers from `src/lib/tools/`.

The trimmer continues to pass its full test + smoke suite from the new shared locations before any compress-specific code lands.

Frontend: the trimmer's `pick → probe → proxy → previewUrl` dance gets *re-typed* in `VideoCompress.tsx` for now — same shape, separate component. Extracting a `useVideoSourceWithProxy()` hook is deferred until a third video tool justifies it (per [../CLAUDE.md](../CLAUDE.md) "Scope discipline").

## Edge cases

- **Slider at max can produce a file larger than the source** for already-heavily-compressed inputs. No special handling — the user picks a lower position. UI copy stays "Smaller file ←→ Higher quality" rather than promising a number.
- **Silent video.** Drop the `-map 0:a?` (same shape as trimmer); `-c:a copy` not emitted when there's no audio stream.
- **Multi-audio sources.** `-map 0:a -c:a copy` keeps every audio track bit-perfect — better than the trimmer's v1 (which encodes all tracks with the first track's codec).
- **Multi-video sources.** Map only `0:v:0`, same as trimmer.
- **HDR sources.** Color metadata not preserved — inherits the trimmer's existing BACKLOG follow-up; not re-listed.
- **Source codec outside the mirror table.** Falls back to libx264 / H.264 CRF range.
- **Subtitle / data / attachment streams** dropped, same as trimmer.
- **Pre-encode cancel** (during proxy transcode) returns to idle without an error banner; mid-encode cancel unlinks the partial output.

## Tile

- Category: `video`
- Color: `violet` (distinct from trimmer's `rose` and format-converter's `teal`).
- Name: "Compress"
- Description: "Re-encode a video at a chosen quality to produce a smaller file."

## Acceptance

- [ ] Hoist refactor lands; trimmer continues to pass `cargo test -p multitool-core --all-targets` + its existing Vitest/component/Playwright lanes from the new shared-module locations.
- [ ] Pick → preview-proxy → compress → reveal flow works end-to-end for mp4 (H.264/AAC), mkv (HEVC), and webm (VP9/Opus) sources in `pnpm tauri dev`.
- [ ] Slider at midpoint produces a clearly smaller file than the source on a typical capture (rough sanity, not a hard bound).
- [ ] Slider at min produces a small-but-still-watchable output (no garbled frames, no audio dropouts — audio is stream-copied so it's bit-perfect anyway).
- [ ] Mid-encode cancel leaves no partial output on disk.
- [ ] Same-name collision lands at `{stem}_compressed (1).{ext}`.
- [ ] Rust pure (`compress.rs`) ≥80% line coverage; orchestrator (`job.rs`) tests cover cancel, collision via `unique_path`, missing input, garbage input, silent-video pathway, unknown-codec fallback.
- [ ] Component smoke test asserts the slider renders, forwards the value to the wrapper, surfaces the error envelope on a rejected `compressVideo`.
- [ ] **Manual smoke session before the Playwright spec** ([memory/feedback_manual_smoke_before_e2e.md]).
- [ ] One Playwright happy-path spec at `tests/e2e/video-compress.spec.ts`.

---

## Plan (commit-sized)

Phase 1 is a pure refactor: the trimmer must keep passing its full test + smoke suite from the new shared-module locations before any compress-specific code lands. Phase 2 + 3 add the new tool. Phase 4 is smoke → e2e → ship per the playbook.

Flip `pending → done` on each row in-place after the commit lands, paste the SHA, and add a one-line gotcha note if any ([memory/feedback_update_working_doc_per_commit.md]).

| # | Commit | Status | SHA | Notes |
| --- | --- | --- | --- | --- |
| 1 | `refactor(video): hoist preview proxy to multitool_core::video` | done | `82d388c` | Pure rename. `cargo test -p multitool-core --all-targets` stayed at 342 passed; only the proxy tests changed module path to `video::proxy::tests`. Trimmer re-exports `generate_proxy` from `crate::video::proxy` so the shell shim's `multitool_core::tools::video_trimmer::generate_proxy` import path is unchanged. |
| 2 | `refactor(video): hoist codec mirror tables to multitool_core::video` | done | `9e1fe3c` | Final API: `video_crf_range(codec) -> (best, worst)` + `video_encoder_flags(codec, crf) -> Vec<String>` + `audio_encoder_flags(codec) -> &'static [&'static str]`. Trimmer's `build_reencode_args` now calls `video_crf_range(codec).0` + `video_encoder_flags(codec, crf)` so the `best` slot is the single source of truth for both tools — the alternative (a `trimmer_video_encoder_flags` re-export) duplicates the codec list across the trimmer and the future compress strategy fn. VP8 deferred from "scale bitrate too" to "fixed `-b:v 1M`" (only the trimmer uses VP8 in v1; matching the existing behavior keeps the refactor pure). 346 tests pass (was 342 — net +4 from the new shared-module tests). |
| 3 | `refactor(video): hoist preview-proxy Tauri commands to shared module` | done | `20f3f02` | Commands now app-level (registered next to `cancel_job` + `allow_media_preview`, not as trimmer commands). Trimmer's TS wrapper unchanged — command names stayed put. 346 Rust + 202 Vitest tests still green. |
| 4 | `feat(video-compress): add core convert + job` | pending | — | New `multitool_core/tools/video_compress/{convert.rs,job.rs,mod.rs}`. `Opts { quality: f64 }`; arg builder mirrors the trimmer's shape minus `-ss`/`-t`, with `-c:a copy` and the CRF interpolated per source codec via `multitool_core::video::reencode`. Tests on `convert.rs` (arg shape, CRF mapping at 0/0.5/1, silent-video path, VP8 special case, fallback codec). Tests on `job.rs` (happy path, cancel, collision, missing input, garbage input, on_progress error). Register module in `tools/mod.rs`. |
| 5 | `feat(video-compress): add Tauri shim` | pending | — | `src-tauri/src/tools/video_compress/mod.rs` with `compress_video` command (uses `run_streaming_job` like the trimmer). Register in `src-tauri/src/tools/mod.rs`. |
| 6 | `feat(video-compress): add TS IPC wrapper` | pending | — | `src/lib/tools/videoCompress.ts` (`Opts`, `Progress`, `JobResult`, `compressVideo`) + Vitest tests mocking `@tauri-apps/api/{core,event}` (covers invoke args, progress filter by jobId, listener unsubscribe on both paths, abort → `cancel_job`, error envelope round-trip). |
| 7 | `feat(video-compress): add tool component + register` | pending | — | `src/tools/video-compress/{VideoCompress.tsx, types.ts, index.ts}` — picker/proxy dance re-typed (no shared hook yet), single shadcn `Slider`, player from the trimmer minus the scrubber. Register in `src/tools/registry.ts`. Extend `src/app/Dashboard.test.tsx` to assert the new tile. Component test on `VideoCompress.tsx` (slider value forwarded, error envelope renders, cancel aborts the signal). May need `pnpm dlx shadcn add slider` if the primitive isn't already generated. |
| 8 | — | pause | — | **STOP for manual smoke session** before writing Playwright. Specific scenarios listed in §"Manual smoke checklist" below. |
| 9 | `test(video-compress): playwright happy-path e2e` | pending | — | `tests/e2e/video-compress.spec.ts` + mock extensions under `tests/e2e/mocks/` if needed. |
| 10 | `chore(video-compress): ship — DECISIONS entry, delete working doc` | pending | — | Drop any non-obvious build choices into [DECISIONS.md](../DECISIONS.md) (mostly: the slider→CRF table + why we stream-copy audio). Delete this file. |

### Manual smoke checklist (step 8)

Run in `pnpm tauri dev`:

1. **Golden:** pick an mp4 (H.264/AAC), drag slider to midpoint, compress, reveal in folder — output is smaller, plays cleanly, audio is bit-identical.
2. **Slider extremes:** same source at slider=0 → small + blocky-but-watchable; slider=1 → may be *larger* than source (expected on a low-bitrate source). No crash either way.
3. **Codec variety:** repeat once each on an mkv (HEVC) source and a webm (VP9/Opus) source. Confirm each output keeps its source's container + video codec family.
4. **Silent video:** pick a clip with no audio stream; confirm the compressor doesn't crash on the missing `-c:a copy` path and the output has no audio.
5. **Mid-encode cancel:** start a compress on a long-ish clip, hit Cancel mid-progress, confirm no `{stem}_compressed.{ext}` (or `(1)`) is left in the source directory.
6. **Same-name collision:** keep the output from step 1, run the same source again with the same slider, confirm the second run lands at `{stem}_compressed (1).{ext}` and the first is untouched.
7. **Picker → reveal flow:** "Open output folder" highlights the new file in the OS file manager.
