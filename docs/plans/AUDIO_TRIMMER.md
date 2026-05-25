# Tool: Audio Trimmer

> Ephemeral working doc, per [../ADDING_A_TOOL.md](../ADDING_A_TOOL.md). Phase 1 = this brief, written 2026-05-25 by a session that is now handing off. Phase 2 (task plan) is sketched at the bottom but unconfirmed — **a fresh session should walk the "Decisions to confirm" list with the user before fleshing out the commit-sized plan**. Deleted when the tool ships.

## Summary

Trim a single audio file to a `[start_ms, end_ms]` range, with optional linear fade-in / fade-out. Output preserves the source format and lands next to the input as `{stem}_trimmed.{ext}`.

## Inputs

- **One** audio file picked via a new single-select picker (`pickAudioFile` in [src/lib/system.ts](../../src/lib/system.ts)). `null` on cancel; never empty.
- Accepted formats: same set as the Audio Format Converter — `mp3 / wav / flac / ogg / oga / m4a / mp4 / aac / aiff / aif / caf / mkv / webm`. The Rust side re-validates via the existing `decode_to_pcm` path so a renamed file routes through skip/error rather than crashing.

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `start_ms` | `u64` | `0` | Trim start in milliseconds from the beginning of the source. |
| `end_ms` | `u64` | source duration | Trim end. Clamped to source duration; rejected if `<= start_ms`. |
| `fade_in_ms` | `u32` | `0` | Linear amplitude ramp 0→1 over the first N ms of the trimmed region. `0` = no fade. Clamped to `end_ms − start_ms` − `fade_out_ms`. |
| `fade_out_ms` | `u32` | `0` | Linear amplitude ramp 1→0 over the last N ms. `0` = no fade. Same clamp. |

Output format is **not** an option — it's always the source format. If users want a format change too, they pipe through the Audio Format Converter after.

## Output

- **Location:** next to the input file ([ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions)).
- **Naming:** `{stem}_trimmed.{ext}`.
- **Duplicate handling:** `multitool_core::fs::unique_path` — `song_trimmed (1).mp3`, etc.

## UX flow

Dashboard → Audio Trimmer tile → state machine:

- `idle` → "Select audio file" button
- `picked` → file name, total duration, start/end markers, fade inputs, **Trim** + **Pick different file** + **Preview** buttons
- `running` → progress indicator (single file, so likely just an indeterminate spinner) + Cancel
- `done` → "Trimmed to {final}_trimmed.{ext}" + **Open output folder** + **Trim another**

`Escape` returns to the dashboard from any state, matching the other tools.

## Edge cases

- `start_ms == end_ms` or `start_ms > end_ms` → reject pre-encode with a `ProcessingFailed` carrying a clear message. UI also disables the Trim button.
- `end_ms > source duration` → clamp to source duration silently (consistent with the existing "values outside bounds get snapped" policy).
- `fade_in_ms + fade_out_ms > (end_ms − start_ms)` → clamp each fade to half the trim length, emit a warning.
- Source has no decodable samples (empty file, garbage, encrypted MP4 etc.) → routed through `decode_to_pcm`'s existing `UnsupportedFormat` path.
- Multi-channel sources: trim and fade apply per-frame (every channel is faded equally). No channel-mode option in v1 — output keeps source layout.
- Source-format constraints already validated by `decode_to_pcm` for input. For output, we re-encode at source sample rate, which by definition is one the source produced — so MP3's restricted sample-rate set is automatically satisfied for MP3 round-trips.
- Cancellation between decode and encode is fine (token-checked). Mid-encode cancel inherits the same v1 limitation as the Audio Format Converter (encoders take the full PCM buffer at once).

## Acceptance

- [ ] Rust unit tests on the pure trim/fade function: range correctness, fade math (verify fade-in starts at amplitude 0, fade-out ends at 0), clamp behaviour, multi-channel preservation. ≥80% line cov per [ARCHITECTURE.md §4](../ARCHITECTURE.md#4-testing-strategy).
- [ ] Orchestrator tests: happy path, invalid range, missing input, cancellation, `unique_path` collision.
- [ ] Vitest on the IPC wrapper at `src/lib/tools/audioTrimmer.ts`.
- [ ] Vitest on the React component: slider drag updates, fade inputs, Trim button disabled when range is invalid, pre-listen button.
- [ ] Playwright happy-path e2e.
- [ ] CI green on the three-OS matrix. **No new build deps expected** — reuses the existing audio stack from the Audio Format Converter (Symphonia, claxon, hound, flacenc, mp3lame-encoder, vorbis_rs).

---

## Decisions to confirm

A fresh session picking this up should **walk these with the user before expanding Phase 2 below**. Recommendations are based on the same trade-offs as the Audio Format Converter session.

### 1. Pre-listen path

The brief says "pre-listen before trimming." Two ways:

- **(A) HTML5 `<audio>` element with the source file** via Tauri's asset protocol, JS-controls for play/pause at the current `[start_ms, end_ms]` markers. Doesn't preview the fade-in/out. Simplest; gets ~80% of the value.
- **(B) Tauri command that produces a temp file with the trimmed+faded output**, played via `<audio>`. Previews fades accurately. Heavier (one encode per preview click).

**Recommending (A) for v1.** Faster, simpler, and "did I pick the right region?" is the dominant question — fades are typically dialed in by ear after a successful trim.

### 2. Drag markers vs. timestamp inputs

- **(A) Slider + numeric inputs both.** Visual drag for "find the region by ear," numeric inputs for precision.
- **(B) Numeric inputs only.** Skip the slider widget entirely; users type or step `MM:SS.ms`.
- **(C) Waveform display with drag handles.** Decode source to PCM, downsample for display, draw on canvas. Significantly more work.

**Recommending (A).** (C) is a great future polish item — leave it as a v2 follow-up.

### 3. Encoder reuse from `audio_format_converter::convert`

The trimmer's encode-back-to-source-format step is exactly what the Audio Format Converter already does for those four formats. Today those `encode_wav` / `encode_flac` / `encode_mp3` / `encode_ogg_vorbis` functions are `pub(super)` in [`multitool-core/src/tools/audio_format_converter/convert.rs`](../../src-tauri/multitool-core/src/tools/audio_format_converter/convert.rs).

- **(A) Refactor into a shared module** — `multitool-core/src/audio_codecs/{decode,encode}.rs`, both tools depend on it. Touches the audio_format_converter module but doesn't change its public API. Future audio tools (compress, concat) reuse the same surface.
- **(B) Copy-paste into the trimmer's module.** Lighter touch; risk of drift if one tool fixes a bug the other doesn't.
- **(C) Pull through the converter via `convert_one`** — the trimmer would write its trimmed PCM to a temp file as WAV, then invoke the converter. Awkward, double-encode, not recommended.

**Recommending (A).** This is the second audio tool; pulling the shared bits up now pays off immediately and again for every future audio tool. Worth a DECISIONS entry ("audio codec helpers live in `multitool_core::audio_codecs`").

### 4. Asset-protocol scope for `<audio>` preview

If we pick (A) on the pre-listen question, the picked audio file needs a per-pick asset-scope grant — same model as [`src-tauri/src/asset_scope.rs`](../../src-tauri/src/asset_scope.rs)'s `allow_image_preview`. Two ways:

- **(A) Generalize** — rename/extend the existing command to `allow_media_preview` with a wider extension allowlist (images + audio). One asset-scope command, one Vitest mock seam.
- **(B) Add a sibling** — `allow_audio_preview` next to `allow_image_preview`, duplicating the per-path scope grant. Cleaner separation; modest duplication.

**Recommending (A).** Asset-scope grants are about "did the user pick this file?" not about media type. The extension-allowlist server-side validation already lives there; widening it is one line + a test.

### 5. Output naming when source is in a subfolder

The other tools all output `{stem}_{suffix}.{ext}` directly next to the input. The audio trimmer should do the same — confirmed by [ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions). No question, just calling it out for completeness.

### 6. Pre-listen "loop the region" behaviour

When the user clicks Preview, should playback:

- **(A) Loop the `[start_ms, end_ms]` region** until they hit Stop / Pause? (Matches what DAWs do.)
- **(B) Play once and stop.**

**Recommending (A).** Looping is more useful for "find the right boundary" iteration. JS-side: seek to start, play, on `timeupdate` check if current >= end → seek to start. Trivial.

---

## Phase 2 — task plan (commit-sized) [unconfirmed]

The shape below assumes decisions 1A, 2A, 3A, 4A, 6A. **A fresh session should rebuild this list after confirming.** Numbers and the per-commit `**Convention.**` from the Audio Format Converter doc apply — update the plan in-place after every commit with the SHA and a one-line gotcha note.

| # | Commit | Status |
| --- | --- | --- |
| 0 | **Pre-work:** Refactor `audio_format_converter::convert`'s encoders + `decode_to_pcm` into `multitool_core::audio_codecs::{decode, encode}`. No behaviour change; the converter module re-routes through the new shared surface. All existing tests still pass. DECISIONS entry: "audio codec helpers live in `multitool_core::audio_codecs`". | pending |
| 1 | `feat(audio-trim): scaffold audio_trimmer module + frontend tile` — Rust stubs (`convert.rs`, `job.rs`, `mod.rs`), Tauri command shim, IPC wrapper, placeholder React component, registry entry. New `pickAudioFile` (single-select) in `src/lib/system.ts`. `allow_media_preview` generalisation of `allow_image_preview` lands here too. | pending |
| 2 | `feat(audio-trim): pure trim + fade` — `convert.rs::trim_and_fade(buf, start_ms, end_ms, fade_in_ms, fade_out_ms) -> AppResult<AudioBuffer>`. Unit tests on each case (range correctness, fade math, clamp warnings, multi-channel). | pending |
| 3 | `feat(audio-trim): wire encode + orchestrator` — call into `audio_codecs::encode_*` based on source extension to round-trip the source format. `job.rs::run_job` single-file pipeline. Tests for happy path, invalid range, missing input, cancellation, `unique_path` collision. | pending |
| 4 | `feat(audio-trim): React UI — slider + numeric inputs + fade fields + Preview button` — drag-to-set markers, MM:SS.ms numeric inputs (round-trip with the slider), fade-in/out ms inputs, **Preview** wires HTML5 `<audio>` with loop-from-start logic on `timeupdate`. Vitest covers slider sync, range validation, preview play/pause toggle. | pending |
| 5 | `test(audio-trim): Playwright e2e happy path` — pick → set markers → trim → done. Mocks the IPC + asset-protocol calls. | pending |
| 6 | `docs(audio-trim): DECISIONS entry + delete working doc + BACKLOG cleanup` — record the chosen pre-listen approach + waveform-as-followup, audio codec helpers location, anything else surprising. Delete this doc. Remove "Audio trim" line from `docs/plans/BACKLOG.md`. | pending |

### Source organization

```
src-tauri/multitool-core/src/
├── audio_codecs/
│   ├── mod.rs
│   ├── decode.rs   # decode_to_pcm + decode_flac_with_claxon (moved from audio_format_converter)
│   └── encode.rs   # encode_wav / encode_flac / encode_mp3 / encode_ogg_vorbis (moved)
└── tools/
    ├── audio_format_converter/   # now thin — types + apply_channel_mode + convert_one + run_job
    └── audio_trimmer/
        ├── convert.rs   # trim_and_fade
        ├── job.rs       # single-file orchestrator
        └── mod.rs

src-tauri/src/tools/audio_trimmer/mod.rs    # Tauri command shim
src/lib/tools/audioTrimmer.ts               # IPC wrapper
src/tools/audio-trimmer/
├── AudioTrimmer.tsx
├── AudioTrimmer.test.tsx
├── index.ts
└── types.ts
```

### Tile color

`violet` or `teal` from the `TileColor` union — both unused. `violet` recommended (visually distinct from the existing `emerald` Audio Format Converter tile in the same Audio section).

### Shared-surface edits expected

Per [ADDING_A_TOOL.md](../ADDING_A_TOOL.md), narrow deliberate edits:

- `src/tools/registry.ts`: add the new tool.
- `src/app/Dashboard.test.tsx`: assert the new tile + the Audio section now has two tiles.
- `src/lib/system.ts`: `pickAudioFile` (single-select) + (decision-4-A) update of `allowImagePreview` → `allowMediaPreview` with a wider allowlist. Update the audio-format-converter component if the wrapper rename happens.
- `src-tauri/src/asset_scope.rs`: rename/extend per decision 4.
- `src-tauri/src/tools/mod.rs`: register the new Tauri command.
- `multitool-core/src/lib.rs`: re-export `audio_codecs` (if pre-work commit 0 lands).
- `multitool-core/src/tools/audio_format_converter/`: re-route through `audio_codecs::*` (no public API change).

### Manual CI

Run `gh workflow run ci.yml --ref feat/audio-trimmer` once after commit 4 lands (UI/orchestration). The encoder/decoder code paths are unchanged from the Audio Format Converter's already-CI-verified state.
