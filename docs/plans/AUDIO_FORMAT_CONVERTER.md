# Tool: Audio Format Converter

> Ephemeral working doc, per [../ADDING_A_TOOL.md](../ADDING_A_TOOL.md). Phase 1 = this brief. Phase 2 (added below) = a commit-sized task plan, drafted after you confirm the brief. Deleted when the tool ships.

## Summary

Batch-convert one or more audio files from any supported source format to a chosen target format. Mirrors the **Image Format Converter** tool in shape: skip+continue per file, per-file progress, output next to input.

## Decoding strategy

[`symphonia`](https://github.com/pdeljanov/Symphonia) is the only sensible pick — pure Rust, decode-only, very broad container + codec coverage. Single dependency for all inputs.

**Accepted inputs** (lowercased extension; bytes-sniffing handled by Symphonia):
- `mp3`, `wav`, `flac`, `ogg`, `oga`, `m4a`, `mp4`, `aac`, `aiff`, `aif`, `caf`, `mkv`, `webm`

The picker filter is advisory — like the Image Format Converter, the Rust side re-validates by attempting a decode; renamed files route through skip+continue.

## Encoding strategy — needs your decision

There are only **two** pure-Rust encoders worth using: `hound` (WAV) and `flacenc` (FLAC). Everything else means a vendored C build via `cc-rs` (`mp3lame-encoder`, `vorbis_rs`, `opus`). Unlike libheif, these are self-contained — no codec-plugin runtime dependency, the C source compiles cleanly cross-platform under cc-rs, no system-package install needed on dev/CI/end-user machines. Closest precedent in the repo: none yet, but the printpdf "default-features = false to trim transitive native deps" approach is the closest spiritual analog.

**Three coverage levels** (pick one — answer below in the "Decisions to confirm" section):

- **A — pure Rust only.** Outputs: **WAV, FLAC**. Zero new build steps; cleanest. Limited if a user wants to ship MP3/OGG.
- **B — pure Rust + LAME + Vorbis.** Outputs: **WAV, FLAC, MP3, OGG (Vorbis)**. Two new crates with vendored C builds. Covers the formats 95% of users actually want to write.
- **C — B plus Opus.** Outputs: **WAV, FLAC, MP3, OGG, Opus**. Three vendored C builds. Diminishing return — Opus is great but niche on the desktop.

Recommendation: **B**. Useful coverage, manageable build surface, no system-dep trap like HEIC.

## Inputs

- One or more audio files picked via a new `pickConvertibleAudio()` in [src/lib/system.ts](../../src/lib/system.ts). Multi-select. Each picker confirmation **replaces** the staged list (same as Image Format Converter — confirmed in your prompt).
- No drag-and-drop yet (the dashboard-wide DnD is on the UX backlog).

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `target_format` | enum (`wav` \| `flac` \| `mp3` \| `ogg` \| `opus`) | `mp3` | Variants determined by chosen coverage level above. |
| `mp3_bitrate_kbps` | u32 (96…320) | `192` | Only shown for MP3. CBR via LAME's `set_brate`. VBR added later if requested. |
| `vorbis_quality` | f32 (−1.0…10.0) | `5.0` | Only shown for OGG (Vorbis). Standard Xiph quality scale. |
| `opus_bitrate_kbps` | u32 (32…256) | `128` | Only shown for Opus. |
| `flac_compression_level` | u32 (0…8) | `5` | Only shown for FLAC. Higher = smaller file, slower. |
| `wav_bit_depth` | enum (`16` \| `24` \| `32f`) | `16` | Only shown for WAV. `32f` = 32-bit float. |
| `sample_rate` | enum (`source` \| `44100` \| `48000` \| `22050`) | `source` | `source` = pass-through. Anything else resamples (linear; rubato added later if quality demands). |
| `channels` | enum (`source` \| `mono` \| `stereo`) | `source` | `mono` = downmix; `stereo` = upmix mono to L=R. |

UI keeps options outside `ViewState` so user choices survive idle ↔ staging transitions, same pattern as Image Format Converter.

Clamping policy: same as Image Format Converter — values outside the bounds are silently moved to the nearest bound. UI clamps for UX; encoder also clamps to defend against bad callers.

## Output

- **Location:** next to the input file (same as every other tool — [ARCHITECTURE §3.3](../ARCHITECTURE.md#33-file-io-conventions)).
- **Naming:** `{stem}.{target_ext}`. Extensions: `wav` → `.wav`, `flac` → `.flac`, `mp3` → `.mp3`, `ogg` → `.ogg`, `opus` → `.opus`.
- **Duplicate handling:** via `multitool_core::fs::unique_path` — same-format conversion produces `song (1).mp3`, etc.

## UX flow

Dashboard → Audio Format Converter tile → state machine identical to Image Format Converter:

- `idle` → "Select audio files" button
- `staging` → list of staged filenames (no thumbnail / no preview — confirmed in your prompt), target-format radio, per-format option block, **Convert** + **Select different audio** buttons
- `running` → per-file progress (`3 / 12 — song.mp3`), **Cancel** button
- `done` → success/skip counts, skipped-files details, **Open output folder** + **Convert another**

No asset-protocol grant — we're not rendering audio in the webview. No `<audio>` preview either (per your spec).

## Edge cases

- **Multi-stream input**: pick the default audio track. Symphonia exposes the default via track flags.
- **Variable channel layouts** (e.g. 5.1, 7.1): if the target encoder doesn't support N>2 channels (LAME mono+stereo; libvorbis arbitrary up to 8; libopus 1+2), downmix to stereo with a per-file warning.
- **Source = target format**: still works; `unique_path` produces a `(1)` suffix so the original isn't clobbered. Lossy → lossy same-format is a transcode (quality loss) — a per-file warning surfaces this.
- **Tags / metadata**: not preserved in the first cut. Tracked as a follow-up (warning emitted if source had Vorbis comments / ID3 frames).
- **DRM / encrypted streams**: rejected by Symphonia → routed to `AppError::UnsupportedFormat`; lands in the skipped list.
- **Very large files** (>1 GB): stream-decode + stream-encode in chunks; never buffer the full PCM. Memory cap target: <100 MB peak regardless of input size.
- **Cancellation**: tested between files (orchestrator-level) **and** between PCM chunks within a file (per-file). The per-chunk hook is checked every ~50 ms of audio; cancelled mid-encode leaves the partial output file deleted, not orphaned.

## Acceptance

- [ ] Unit tests on the pure encode/transcode function: every accepted output format encodes a round-trip-decodable file. ≥80% line cov per [ARCHITECTURE §4](../ARCHITECTURE.md#4-testing-strategy).
- [ ] Unit tests on the orchestrator: happy path, mid-batch unsupported skip, missing input skip, cancel-between-files, cancel-mid-file, `unique_path` collision.
- [ ] Vitest tests on the IPC wrapper at `src/lib/tools/audioFormatConverter.ts`.
- [ ] Vitest + Testing Library tests on the React component: option visibility (MP3 shows bitrate, OGG shows quality, etc.), error envelope, cancel.
- [ ] Playwright happy-path e2e: pick → format → convert → done.
- [ ] `pnpm tauri build --no-bundle` passes on Linux locally; CI green on all three OSes (this is the load-bearing check — new C-compiled deps regress here first).

---

## Confirmed decisions

1. **Coverage level B**: outputs are **WAV, FLAC, MP3, OGG Vorbis**. No Opus in v1.
2. **Default target format**: **MP3** (most-asked).
3. **Per-format option blocks**: visible only when the matching target is selected (mirrors Image Format Converter's "alpha handling appears only when target is alpha-less").
4. **Sample rate**: **passthrough only** in v1. No user-facing rate knob, no resampler dep.
5. **Tag preservation**: **deferred** to a follow-up. v1 writes untagged outputs.

### Build-toolchain note (load-bearing)

`mp3lame-sys` (transitive dep of `mp3lame-encoder`) builds LAME 3.100 with **GNU autotools on Unix** (`autoconf`, `automake`, `libtool`) and `cc` on Windows. CI impact, per OS:

- **Ubuntu**: autoconf/automake/libtool are part of the `ubuntu-latest` image — no change needed.
- **macOS**: must `brew install autoconf automake libtool` before `cargo build`. New step in [.github/workflows/ci.yml](../../.github/workflows/ci.yml) and [release.yml](../../.github/workflows/release.yml).
- **Windows**: cc only — no change.

`vorbis_rs` (and its `aotuv_lancer_vorbis_sys` core) uses cc-rs exclusively → no autotools requirement. Everything else (`hound`, `flacenc`, `symphonia`) is pure Rust.

This needs a DECISIONS.md entry once Phase 2 lands — "Audio: mp3lame-sys autotools requirement on macOS CI".

---

## Phase 2 — task plan (commit-sized)

Status updates land in this section as commits ship. The PR is one logical change; commits are checkpoints within it.

**Convention.** Update this table in-place after each commit lands — flip `pending → done`, paste the commit SHA, and add a one-line note on anything surprising (a workaround, a gotcha caught during testing, a follow-up). A fresh Claude (or human) session reads this doc to pick up exactly where the last one stopped — without rummaging through `git log`. Same rule applies to any working doc under `docs/plans/`.

| # | Commit | Status |
| --- | --- | --- |
| 1 | `feat(audio): scaffold audio_format_converter module + cargo deps` — add `audio` to `ToolCategory`, stub Rust modules (`convert.rs`, `job.rs`, `mod.rs`) on both crates, register the (no-op) Tauri command, stub the frontend folder with a placeholder component, add `pickConvertibleAudio` to `src/lib/system.ts`, update Dashboard test. Goal: `cargo build` succeeds with all new vendored-C deps compiling. **Riskiest commit — proves the toolchain story before any encoder code.** | **done** — `8351b63`. All five lefthook gates passed (fmt, clippy `-D warnings`, prettier, typecheck, eslint). **Linux only**; Mac / Windows CI not yet exercised (autotools brew step for `mp3lame-sys` not in CI until commit 4). |
| 2 | `feat(audio): symphonia-based decode to interleaved PCM` — `convert.rs::decode_to_pcm(bytes, source_ext) -> AudioBuffer` returning `{ samples: Vec<f32>, channels: u16, sample_rate: u32 }`. Unit tests for WAV / MP3 / FLAC / OGG fixtures. | **done** — `218dff6`. Gotcha caught during testing: symphonia 0.6's `copy_to_vec_interleaved` **resizes** (replaces) the destination vec, not appends — confirmed against `symphonia-core/src/audio/mod.rs:285`. The decode loop now copies each packet into a scratch vec and `extend_from_slice`s into the accumulator. 5 fixtures (`tiny_mono.{wav,mp3,flac,ogg}` + `tiny_stereo.wav`) generated via ffmpeg, ~95 KB total. |
| 3 | `feat(audio): pure-Rust WAV + FLAC encoders` — `encode_wav` (hound), `encode_flac` (flacenc). Round-trip tests (decode the encoded output, verify dims). | pending |
| 4 | `feat(audio): MP3 + OGG Vorbis encoders` — `encode_mp3` (mp3lame-encoder, CBR), `encode_ogg` (vorbis_rs). Round-trip tests. Workspace-level CI yml change for macOS brew step lands here. | pending |
| 5 | `feat(audio): channel + sample-rate compatibility` — downmix N>2 → stereo with warning, mono ↔ stereo per `channels` option, skip files when source rate is outside the encoder's accepted set (MP3 only — Vorbis/FLAC/WAV are flexible). | pending |
| 6 | `feat(audio): batch orchestrator with skip+continue` — `job.rs::run_job`. Mirror Image Format Converter's `Progress` / `JobResult` shape. Tests: happy/skip/missing/cancel-between/cancel-mid-file/`unique_path` collision/`on_progress` error. | pending |
| 7 | `feat(audio): Tauri command shim + IPC wrapper + types` — `src-tauri/src/tools/audio_format_converter/mod.rs`, `src/lib/tools/audioFormatConverter.ts` + Vitest. Register handler in `tools/mod.rs`. | pending |
| 8 | `feat(audio): React tool — picker, options form, progress, done state` — `AudioFormatConverter.tsx`, register in `src/tools/registry.ts`, Dashboard test assertions. Component-level Vitest. | pending |
| 9 | `test(audio): Playwright e2e happy path` — pick → MP3 → convert → done. | pending |
| 10 | `docs(audio): DECISIONS entries + tool plan deletion` — record (a) mp3lame-sys autotools on macOS CI, (b) audio-stack rationale (Symphonia for decode, format-specific encoders, passthrough sample rate). Delete this working doc on merge per ADDING_A_TOOL.md. | pending |

### Source organization

```
src-tauri/multitool-core/src/tools/audio_format_converter/
├── convert.rs   # pure transcode (decode → optional channel ops → encode)
├── job.rs       # batch orchestrator
├── mod.rs       # re-exports
└── tests/fixtures/audio/  # short WAV/MP3/FLAC/OGG fixtures (≤ a few seconds, < 50 KB each)

src-tauri/src/tools/audio_format_converter/mod.rs   # Tauri command shim

src/lib/tools/audioFormatConverter.ts               # IPC wrapper
src/tools/audio-format-converter/
├── AudioFormatConverter.tsx
├── AudioFormatConverter.test.tsx
├── index.ts
└── types.ts
```

### Shared-surface edits expected

Per [ADDING_A_TOOL.md](../ADDING_A_TOOL.md), these are the narrow shared edits this tool needs (deliberate, not violations):

- `src/tools/registry.ts`: add `"audio"` to `ToolCategory`, `{ id: "audio", label: "Audio" }` to `toolCategories`.
- `src/app/globals.css`: probably reuse `--tile-emerald` (unused so far) — no new color tokens unless we want them.
- `src/lib/system.ts`: new `pickConvertibleAudio()` matching `pickConvertibleImages` shape.
- `src-tauri/src/asset_scope.rs`: **no change** — audio doesn't render via `<audio>` previews in v1.
- `src/app/Dashboard.test.tsx`: assert the new tile + new section heading.
- `.github/workflows/{ci,release}.yml`: macOS step `brew install autoconf automake libtool`.

### Tile color

Pick `emerald` (unused so far; visually distinct from rose/amber/sky already in the dashboard). The audio-trimmer follow-up gets a different color from the unused set.
