# Tool: Audio Trimmer

> Ephemeral working doc, per [../ADDING_A_TOOL.md](../ADDING_A_TOOL.md). Phase 1 brief written 2026-05-25; decisions confirmed + Phase 2 commit-sized plan locked in 2026-05-25. Deleted when the tool ships.

## Summary

Trim a single audio file to a `[start_ms, end_ms]` range, with optional linear fade-in / fade-out. Output preserves the source format and lands next to the input as `{stem}_trimmed.{ext}`.

## Inputs

- **One** audio file picked via a new single-select picker (`pickAudioFile` in [src/lib/system.ts](../../src/lib/system.ts)). `null` on cancel; never empty.
- **Accepted formats:** `mp3 / wav / flac / ogg / oga` — the four formats this codebase has an encoder for. (The Audio Format Converter decodes a wider set via Symphonia, but we have no encoders for m4a/aac/aiff/caf/mkv/webm, so the trimmer can't round-trip those back to the source format. The picker filter constrains the surface; the Rust side re-validates via `decode_to_pcm`.)

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `start_ms` | `u64` | `0` | Trim start in milliseconds from the beginning of the source. |
| `end_ms` | `u64` | source duration | Trim end. Clamped to source duration; rejected if `<= start_ms`. |
| `fade_in_ms` | `u32` | `0` | Linear amplitude ramp 0→1 over the first N ms of the trimmed region. `0` = no fade. Clamped to `(end_ms − start_ms) / 2` when fade-in + fade-out overlaps. |
| `fade_out_ms` | `u32` | `0` | Linear amplitude ramp 1→0 over the last N ms. `0` = no fade. Same overlap clamp. |

The UI exposes fades as **two checkboxes** (Fade in / Fade out), each toggling a fixed `1000 ms` duration. The Rust `Opts` keep millisecond integers so unit tests can hit edge cases (zero, equal-to-window, overlap) directly; the UI just doesn't surface a duration input.

Output format is **not** an option — it's always the source format. If users want a format change, they pipe through the Audio Format Converter after.

## Output

- **Location:** next to the input file ([ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions)).
- **Naming:** `{stem}_trimmed.{ext}`.
- **Duplicate handling:** `multitool_core::fs::unique_path` — `song_trimmed (1).mp3`, etc.

## UX flow

Dashboard → Audio Trimmer tile → state machine:

- `idle` → "Select audio file" button
- `picked` → file name, total duration, **waveform** with drag-to-set markers, MM:SS.ms numeric inputs (round-tripping with the markers), fade-in/out checkboxes, **Trim** + **Pick different file** + **Preview** buttons
- `running` → indeterminate spinner + Cancel
- `done` → "Trimmed to {stem}_trimmed.{ext}" + **Open output folder** + **Trim another**

`Escape` returns to the dashboard from any state, matching the other tools.

## Edge cases

- `start_ms == end_ms` or `start_ms > end_ms` → reject pre-encode with a `ProcessingFailed`. UI also disables the Trim button.
- `end_ms > source duration` → clamp to source duration silently.
- `fade_in_ms + fade_out_ms > (end_ms − start_ms)` → clamp each fade to half the trim length; emit a warning.
- Source has no decodable samples → routed through `decode_to_pcm`'s `UnsupportedFormat`.
- Multi-channel sources: trim and fade apply per-frame (every channel scaled equally). No channel-mode option in v1 — output keeps source layout.
- MP3 sample-rate validation reuses the converter's existing `LAME_SUPPORTED_RATES` gate via the shared codec module; an MP3 input outside the LAME set surfaces as a `ProcessingFailed`.
- Cancellation between decode and encode is fine (token-checked). Mid-encode cancel inherits the same v1 limitation as the Audio Format Converter.

## Acceptance

- [ ] Rust unit tests on the pure trim/fade function: range correctness, fade math (fade-in starts at amplitude 0, fade-out ends at 0), overlap clamp warning, multi-channel preservation. ≥80% line cov per [ARCHITECTURE.md §4](../ARCHITECTURE.md#4-testing-strategy).
- [ ] Orchestrator tests: happy path, invalid range, missing input, cancellation, `unique_path` collision.
- [ ] Vitest on the IPC wrapper at `src/lib/tools/audioTrimmer.ts`.
- [ ] Vitest on the React component: marker drag updates, numeric input round-trip, fade checkbox toggle, Trim button disabled when range invalid, Preview play/stop toggle.
- [ ] Playwright happy-path e2e.
- [ ] CI green on the three-OS matrix. **No new build deps** — reuses the existing audio stack via the shared `audio_codecs` module introduced in commit 0.

---

## Decisions (confirmed 2026-05-25)

1. **Pre-listen path** — HTML5 `<audio>` element backed by Web Audio API (`AudioBufferSourceNode` + `GainNode`) so fade-in / fade-out can be approximated client-side via gain ramps, *without* a re-encode trip per preview. Web Audio's `decodeAudioData` covers every format we accept. Fades on preview are approximate by design — they reuse the same `fade_*_ms` value (1000 ms when the checkbox is on) the encoder will use, but timing accuracy depends on the browser scheduler; close enough for "did I pick the right region?" iteration.
2. **Drag markers + numeric inputs + waveform** — all three, in this PR. Waveform is rendered client-side on a `<canvas>` from the same Web Audio `AudioBuffer` we already need for the preview path; no Rust waveform command — peaks are computed in JS per bin (min/max f32). Cheap and avoids an extra IPC round-trip.
3. **Encoder reuse** — refactor into `multitool_core::audio_codecs::{decode, encode}`. `decode_to_pcm` + `AudioBuffer` + every `encode_*` fn move to the shared module; the converter re-routes through it without a public-API change. (Commit 0 below.)
4. **Asset-protocol scope** — generalize `allow_image_preview` → `allow_media_preview` with a widened allowlist (existing image extensions + the trimmer's audio set). One command, one Vitest mock seam.
5. **Output naming** — `{stem}_trimmed.{ext}`. Same shape as the other tools.
6. **Preview loop** — yes, loop the `[start_ms, end_ms]` region until Stop. Re-trigger on the `AudioBufferSourceNode`'s `onended`.
7. **Format scope (new this session)** — restrict to wav/mp3/flac/ogg/oga because they're the only formats we can round-trip back to the source format. m4a/aac/aiff/caf/mkv/webm are decode-only via Symphonia.

---

## Phase 2 — task plan (commit-sized) [confirmed]

Each commit ships on its own with green CI. Working doc gets a status flip + SHA + one-line gotcha after every commit (per [feedback_update_working_doc_per_commit](../../../home/yokisama/.claude/projects/-home-yokisama-Projects-multitool/memory/feedback_update_working_doc_per_commit.md), if you can see it).

| # | Commit | Status |
| --- | --- | --- |
| 0 | **Pre-work:** Refactor `audio_format_converter::convert`'s encoders + `decode_to_pcm` into `multitool_core::audio_codecs::{decode, encode}`. No behaviour change — the converter module re-routes through the new shared surface. All existing tests still pass. DECISIONS entry follows in commit 6 (rolled up). _Gotcha: `audio_format_converter/job.rs`'s test mod imported `WavBitDepth` via `super::super::convert`; after the move it has to import via `crate::audio_codecs::encode::WavBitDepth`._ | done · `2cab704` |
| 1 | `feat(audio-trim): scaffold audio_trimmer module + frontend tile` — Rust stubs (`convert.rs`, `job.rs`, `mod.rs`), Tauri command shim, IPC wrapper, placeholder React component, registry entry. New `pickAudioFile` (single-select) in `src/lib/system.ts`. `allow_media_preview` generalisation of `allow_image_preview` lands here too. _Gotcha: `pnpm typecheck` flagged the e2e mock's `pickAudioFile` returning `MOCK_AUDIO_PATHS[0]` because TS noUncheckedIndexedAccess widens that to `string | undefined`; coerce with `?? null`. Also `audioTrimmer.ts`'s unused `AppErrorEnvelope` import — drop or re-export only when consumed._ | done (commit pending) |
| 2 | `feat(audio-trim): pure trim + fade` — `convert.rs::trim_and_fade(buf, start_ms, end_ms, fade_in_ms, fade_out_ms) -> AppResult<(AudioBuffer, Vec<String>)>`. Unit tests on each case (range correctness, fade math, overlap clamp warning, multi-channel). | pending |
| 3 | `feat(audio-trim): wire encode + orchestrator` — call into `audio_codecs::encode_*` based on source extension to round-trip the source format. `job.rs::run_job` single-file pipeline. Tests for happy path, invalid range, missing input, cancellation, `unique_path` collision. | pending |
| 4 | `feat(audio-trim): React UI — waveform + slider + numeric inputs + fade checkboxes + Preview` — Web Audio `decodeAudioData` once on pick; render peaks on `<canvas>`; drag handles for `[start, end]`; MM:SS.ms numeric inputs round-trip with markers; Fade-in / Fade-out checkboxes (1000 ms each); **Preview** wires `AudioBufferSourceNode` + `GainNode` with `linearRampToValueAtTime` for fade approximation and loop-from-start on `onended`. Vitest covers marker↔numeric sync, fade checkbox state, preview toggle. | pending |
| 5 | `test(audio-trim): Playwright e2e happy path` — pick → set markers → trim → done. Mocks the IPC + asset-protocol calls. | pending |
| 6 | `docs(audio-trim): DECISIONS entry + delete working doc + BACKLOG cleanup` — record the chosen pre-listen approach + format-scope rationale + audio codec helpers location. Delete this doc. Remove "Audio trim" line from `docs/plans/BACKLOG.md`. | pending |

### Source organization

```
src-tauri/multitool-core/src/
├── audio_codecs/
│   ├── mod.rs       # AudioBuffer (now pub) + re-exports
│   ├── decode.rs    # decode_to_pcm + decode_flac_with_claxon + symphonia_to_app_err + FLAC_MAGIC
│   └── encode.rs    # WavBitDepth + encode_wav / encode_flac / encode_mp3 / encode_ogg_vorbis
│                    # + their helpers (closest_lame_bitrate, xiph_to_internal_quality, …)
│                    # + validate_mp3_sample_rate + MP3_BITRATE_*/VORBIS_QUALITY_* consts
└── tools/
    ├── audio_format_converter/
    │   ├── convert.rs   # now thin — Opts + TargetFormat + ChannelMode + apply_channel_mode
    │   │                #   + convert_one (delegates decode/encode to audio_codecs)
    │   ├── job.rs       # unchanged
    │   └── mod.rs       # unchanged exports
    └── audio_trimmer/
        ├── convert.rs   # trim_and_fade
        ├── job.rs       # single-file orchestrator
        └── mod.rs

src-tauri/src/tools/audio_trimmer/mod.rs    # Tauri command shim
src/lib/tools/audioTrimmer.ts               # IPC wrapper + Vitest
src/tools/audio-trimmer/
├── AudioTrimmer.tsx
├── AudioTrimmer.test.tsx
├── index.ts
└── types.ts
```

### Tile color

`violet` — distinct from the existing `emerald` Audio Format Converter tile in the same Audio section.

### Shared-surface edits expected

Per [ADDING_A_TOOL.md](../ADDING_A_TOOL.md), narrow deliberate edits:

- `src/tools/registry.ts`: add the new tool.
- `src/app/Dashboard.test.tsx`: assert the new tile + the Audio section now has two tiles.
- `src/lib/system.ts`: `pickAudioFile` (single-select) + rename `allowImagePreview` → `allowMediaPreview` with a widened allowlist.
- `src-tauri/src/asset_scope.rs`: rename `allow_image_preview` → `allow_media_preview` + extend allowlist.
- `src-tauri/src/tools/mod.rs`: register the new Tauri command + the renamed asset-scope command.
- `multitool-core/src/lib.rs`: re-export `audio_codecs` (in commit 0).
- `multitool-core/src/tools/audio_format_converter/`: re-route through `audio_codecs::*` (no public API change).

### Manual CI

Run `gh workflow run ci.yml --ref feat/audio-trimmer` once after commit 4 lands (UI/orchestration). The encoder/decoder code paths are unchanged from the Audio Format Converter's already-CI-verified state.
