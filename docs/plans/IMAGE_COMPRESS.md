# Tool: Image Compress

## Summary

Batch-compress images with a single quality slider. Per-source-format strategy: JPEG stays JPEG, WebP stays WebP, lossless inputs (PNG/BMP/TIFF) convert to lossy WebP under the hood so the slider has meaning regardless of source. Output lands next to each input as `{stem}_compressed.{jpg|webp}`.

> This brief was written ahead of the build in a separate session. The fresh session should: read this top to bottom; confirm scope vs. the open Video Compress PR (#29) is merged into `master` before starting (refactor surfaces from that PR are reused); branch from latest `master` after rebasing this branch onto it. See "Coordination with Video Compress" below.

## Inputs

- Multi-select via `pickConvertibleImages()` — same picker filter the Image Format Converter uses (`png / jpg / jpeg / webp / bmp / tif / tiff`).
- Empty selection → idle state, no-op.

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `quality` | `f64` in `[0.0, 1.0]` | `0.8` | Same wire shape as Video Compress's `Opts`. `0.0` = smallest file (worst quality), `1.0` = highest quality. Rust maps to per-encoder quality 1–100 (both JPEG and WebP use the same 0–100 dialect, so a single `quality_to_q1to100` helper covers both). Out-of-range values clamp. |

No format picker, no EXIF toggle, no metadata knob. EXIF is stripped by side effect of the `image` crate's decode → re-encode (consistent with Image Crop / Image Resize). Document this in DECISIONS so a future feature-bloat impulse doesn't quietly add it back.

## Output

- Location: same directory as the input.
- Naming: `{stem}_compressed.{ext}` where `{ext}` depends on the source-format strategy below.
- Duplicate handling: per [../ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions) — `unique_path` resolves collisions to `{stem}_compressed (1).{ext}`.

### Per-source-format strategy

| Source extension | Output extension | Encoder | Slider meaning |
| --- | --- | --- | --- |
| `.jpg` / `.jpeg` | `.jpg` / `.jpeg` (preserves source spelling) | `image::codecs::jpeg::JpegEncoder` (no new dep) | JPEG quality 1–100 |
| `.webp` | `.webp` | `webp` crate (libwebp, new dep — see DECISIONS entry below) | WebP quality 1–100 |
| `.png` | `.webp` | `webp` crate (lossy WebP supports alpha → safe for PNGs with transparency) | WebP quality 1–100 |
| `.bmp` / `.tif` / `.tiff` | `.webp` | `webp` crate | WebP quality 1–100 |

Rationale for the WebP target on lossless sources:
- WebP supports alpha, so a PNG with transparency round-trips correctly (JPEG would silently flatten alpha onto white — surprising and wrong).
- Lossy WebP is typically smaller than JPEG at equivalent perceptual quality, so the "compress" name stays honest.
- One lossy encoder for the lossless lane means one CRF-style code path, not two.

The output extension change for PNG/BMP/TIFF is visible to the user: the staging row shows the predicted output filename before the run starts.

## UX flow

1. Dashboard → Compress tile (under **Image** category) → empty state with a "Select image files" button.
2. User picks files → staging view:
   - Per-file rows: source filename + size in human-readable units (e.g. `2.4 MB`), arrow, predicted output filename (with the post-strategy extension).
   - Slider at the bottom: a single horizontal range input labeled **"Smaller file"** on the left and **"Higher quality"** on the right — same shape as the Video Compress slider. No number, no codec name.
   - Buttons: "Compress", "Pick different files".
3. Run → progress view: per-file index/total + a progress bar tracking files completed.
4. Done → list of N rows showing `{source} → {output}: {original_size} → {compressed_size} ({delta}%)`. Skipped files (decode failures, unsupported pixel formats) listed separately with their `AppError.message`.
5. Buttons in the done view: "Open output folder" (reveals the first output's folder), "Compress another batch".

Cancellation: between files only — a JPEG/WebP encode for a single image is fast enough that mid-file cancellation isn't useful and would complicate cleanup. Top-of-loop token check; the file in flight finishes, the rest of the batch aborts.

Error envelope: per-file failure routes through `Progress::FileSkipped { source, error }` and the orchestrator continues. A batch-level failure (e.g. output dir not writable on the very first file) returns a top-level `AppError`. Mirrors the Image Format Converter's skip-and-continue shape.

## Slider mapping

Single linear map, no per-format branching:

```
quality_1_to_100 = round(1 + 99 * slider.clamp(0, 1))
```

So `slider=0.0 → q=1`, `slider=1.0 → q=100`, `slider=0.5 → q=50`, `slider=0.8 (default) → q=80`. Both JPEG and lossy WebP accept this same 1–100 dialect, so one helper covers both encoders.

Default `0.8` is the "save for web" sweet spot — visibly smaller than the source for most JPEGs while still indistinguishable at typical viewing sizes. Same default rationale as the Video Compress (midpoint isn't necessarily the right starting point; 0.8 is closer to where a thoughtful user would land after smoke-testing a few clips).

## Reusable bits

From existing code:
- [`multitool_core::image::decode_oriented(source_ext, bytes)`](../src-tauri/multitool-core/src/image/mod.rs) — EXIF-aware decode. Handles all the input formats in this tool's set.
- [`multitool_core::image::RasterFormat::from_extension(ext)`](../src-tauri/multitool-core/src/image/raster_format.rs) — source-format lookup. Use to gate the per-format strategy.
- [`multitool_core::image::tiff_frame_count(bytes)`](../src-tauri/multitool-core/src/image/tiff.rs) — multi-frame TIFF rejection (return `UnsupportedFormat` if > 1 frame); same defense the Crop and Resize tools use.
- [`multitool_core::fs::unique_path`](../src-tauri/multitool-core/src/fs.rs) — collision-resolved output naming.
- The Image Format Converter's batch orchestrator shape (`Progress::FileStarted` / `FileCompleted` / `FileSkipped`, skip-and-continue, cancel between files) — see [`multitool-core/src/tools/image_format_converter/job.rs`](../src-tauri/multitool-core/src/tools/image_format_converter/job.rs).
- The Image Format Converter's staging-UI shape in the frontend — see [`src/tools/image-format-converter/ImageFormatConverter.tsx`](../src/tools/image-format-converter/ImageFormatConverter.tsx).
- [`pickConvertibleImages()`](../src/lib/system.ts) — the picker filter is already correct for this tool's input set.

From the just-shipped Video Compress (assumes its PR has merged before this build starts):
- The single-slider 0..1 wire shape (`Opts { quality: f64 }`) — TS wrapper + Rust types follow the same pattern.
- The "no number, no codec name, just endpoint labels" UI ethos — repeat verbatim.
- The slider-default-is-`0.5` decision is *not* inherited — JPEG/WebP at midpoint is more aggressive than CRF-50 on video, so the Image Compress default lands at `0.8` (see "Slider mapping" above).

## DECISIONS entries to add (during the build)

- **libwebp dep via the `webp` crate.** The existing "WebP output is lossless only" entry pre-empts this — it explicitly says "if lossy WebP is needed, weigh the libwebp dep in a new entry first". This is that entry. Justification: lossy WebP enables both v1 WebP→WebP re-encode AND the PNG/BMP/TIFF→WebP under-the-hood conversion path. Without it, PNG inputs either silently strip alpha (JPEG fallback — wrong) or get rejected (narrow v1 — annoying). The `webp` crate wraps `libwebp-sys`, which compiles libwebp from source via `cc-rs` (no autotools, no system libs required — unlike `mp3lame-sys`'s autotools requirement that needed a brew step on macOS CI). Verify on the first push that CI builds clean across all three runners; if a runner needs an extra step, add it to both `ci.yml` and `release.yml` like the `mp3lame` brew step. License: BSD; bundle-compatible.
- **Per-source-format strategy + extension change for PNG/BMP/TIFF.** Output extension differs from input for the lossless-source lane (PNG/BMP/TIFF → `.webp`). Document so a future "should output preserve extension?" instinct doesn't unwind the strategy. Note the alpha-preservation rationale and the JPEG-flattens-alpha trap.
- **EXIF stripped by side effect of `image` crate decode/re-encode.** Same behaviour as Crop / Resize — pin it explicitly here so a future "preserve EXIF" feature instinct gets a written counter-argument.

## Edge cases

- **PNG with transparency** → WebP output keeps alpha. Verify in smoke: convert a PNG with a transparent background, open the output in an image viewer or paste over a non-white background; the transparency should round-trip.
- **CMYK JPEG** → the `image` crate's JPEG decoder may reject CMYK. Surface per-file as `AppError::UnsupportedFormat` → routes through `Progress::FileSkipped`, batch continues.
- **Multi-frame TIFF** → reject up-front via `tiff_frame_count > 1` → `UnsupportedFormat`. Same as Crop / Resize.
- **Source bytes don't match the extension** (PNG bytes in a `.jpg` file) → `decode_oriented` decodes the bytes regardless. The strategy table is keyed on the *extension*, so an extension-mismatched file still routes through the JPEG lane if it's named `.jpg`. Matches the Crop tool's "extension wins for output, bytes win for decode" rule. Document in code (the source format lookup is by extension, not magic-byte sniffing).
- **Output is larger than input** (re-encoding an already-low-quality JPEG at slider=1 can bloat the file). No special handling — written anyway. The done view's size delta will show the increase; user can re-run at a lower slider position.
- **Animated PNG / WebP** → out of scope for v1. The picker doesn't filter them out (they share extensions with static variants); they'll decode as the first frame via `decode_oriented` and re-encode as a static lossy WebP. Document in DECISIONS; backlog the animated-input handling.
- **Source extension not in the strategy table** (e.g. `.gif`, `.heic`, `.svg`) → not in `pickConvertibleImages()`'s allow-list anyway, so unreachable through the UI. A bad IPC call with such a path errors with `UnsupportedFormat`.
- **Empty file / zero-byte input** → decode error → routes through `FileSkipped`.
- **Source ALREADY matches `{stem}_compressed.{ext}` naming** (rerun on a compressed batch) → `unique_path` produces `_compressed (1).{ext}`, no overwrite.

## Tile

- Category: `image`
- Color: `amber` — distinct from image-category neighbours (Format `sky`, Crop `violet`, Resize `emerald`). Reused from `images-to-pdf` (PDF category) and `audio-extractor` (Video category) — same color in a different category is fine per the registry contract.
- Name: "Compress"
- Description: "Re-encode images at a chosen quality to produce smaller files."

## Coordination with the Video Compress PR

When the fresh session starts:

1. **Confirm PR #29 (Video Compress) has merged into `master`.** If it hasn't, surface that and pause — this brief's "Reusable bits" section assumes the shared `multitool_core::video` + the slider UI pattern are already on `master`. (Video Compress's *Rust* surfaces aren't reused by Image Compress — different family entirely — but the UI ethos + the IPC wrapper shape are.)
2. **Rebase `feat/image-compress` onto the merged `master`.** This branch was cut from `master` at `6e10e89`; after the merge, `master` will be one commit ahead. `git fetch && git rebase origin/master`.
3. Then proceed through the plan below.

## Plan (commit-sized)

Update the plan after every commit, in-place — flip `pending → done`, paste the SHA, add a one-line gotcha if any (per [memory/feedback_update_working_doc_per_commit.md]).

| # | Commit | Status | SHA | Notes |
| --- | --- | --- | --- | --- |
| 0 | `docs(image-compress): brief + plan working doc` | done | 23aaf2b | Doc-only; no consumer code yet. |
| 1 | `feat(image-compress): add libwebp dep via webp crate` | done | 71d4733 | `webp = "0.3"` resolved to 0.3.1; `libwebp-sys` 0.9.6 built via cc-rs on **all three** CI runners (Linux/macOS/Windows) on the first push — no system-dep step needed, libwebp-sys cleaner than `mp3lame-sys` was. |
| 2 | `feat(image-compress): rust core — convert + job` | done | 3565ad8 | 32 tests green. Surprise: `CompressedOutput` returns the **target extension** alongside the bytes (per-source strategy can change it — `png` → `webp` — and JPEG preserves source spelling). The orchestrator threads this into the output filename so the strategy stays a single source of truth. |
| 3 | `feat(image-compress): tauri shim` | done | 2551dfd | One-shot — identical shape to the Image Format Converter shim, no surprises. |
| 4 | `feat(image-compress): TS IPC wrapper` | done | 4e2dc42 | 5 Vitest cases green. `JobResult.outputs[]` exposes per-file `original_bytes` / `compressed_bytes` — distinct shape from `imageFormatConverter`, which doesn't surface successful files separately. |
| 5 | `feat(image-compress): tool component + register tile` | done | 96ba9bf | Surprise: source-size in the staging row needed a backend hop. Added `file_size` Tauri command in `src/system.rs` + `fileSize()` wrapper rather than the `tauri-plugin-fs` dep — single stat lookup doesn't justify a new plugin. Also added shared `formatBytes()` in `src/lib/utils.ts` (SI-decimal, matches OS file managers). 5 component tests green; Dashboard test extended for 4 image tiles + amber binding. |
| 6 | — | pause | — | **STOP for manual smoke session** before writing Playwright. Scenarios listed in §"Manual smoke checklist" below. |
| 6a | `refactor(image): extract shared ImageThumbnail component` | done | 8b4f058 | First smoke round: the text-only file list missed the visual context the other image tools already provided. Extracted the format-converter's thumbnail tile to `src/components/ImageThumbnail.tsx` (with optional `subtitle` slot). Format converter refactored to use it; Images → PDF stays on its hand-rolled tile because of the dnd-kit drag-handle wrapper. |
| 6b | `feat(image-compress): shared thumbnails + reactive WebP-redirect banner` | done | aea1f38 | Adopted `<ImageThumbnail>` in the staging grid (subtitle = size + predicted output filename). Added `<FormatRedirectBanner>` that reactively surfaces above the grid when the picked set contains PNG/BMP/TIFF, with per-format counts. Backlog now carries follow-ups for PNG-via-imagequant + TIFF-in-format compression (deliberately deferred to keep the v1 encoder lane single). **Re-smoke before writing Playwright.** |
| 7 | `test(image-compress): playwright happy-path e2e` | done | 86b7de0 | Spec also asserts the WebP-redirect banner (PNG (1)) and the predicted-output filename rows. Mock `fileSize()` added to `tests/e2e/mocks/system.ts` for the staging size column. Section-filtering by heading sidesteps the Compress-name collision with Video → Compress. All 13 e2e green. |
| 8 | `chore(image-compress): ship — DECISIONS entry, delete working doc` | pending | — | Final DECISIONS entry pinning the per-format strategy + slider default + EXIF behaviour. Delete this file. If smoke or e2e surfaced new BACKLOG items, add them now (likely candidates: animated-PNG/WebP handling; PNG-with-alpha → WebP vs PNG-quantize toggle). |

### Manual smoke checklist (step 6)

Run in `pnpm tauri dev`:

1. **Golden JPEG batch** (5+ JPEG files, mixed sizes from ~100 KB to ~5 MB). Slider at midpoint, compress, "Open output folder". Confirm: each output is smaller than its source, file-size deltas in the done view are accurate, output filenames preserve `.jpg` / `.jpeg` spelling per source.
2. **PNG with transparency** → confirm output is `.webp` and alpha round-trips (open in an image viewer with a non-white background or drag onto a coloured surface).
3. **WebP source** → output stays `.webp`; size shrinks.
4. **BMP / TIFF source** → output is `.webp`. (BMP/TIFF are uncommon as compress inputs; verify the lane works.)
5. **Mixed batch** (JPEG + PNG + WebP in one selection). Confirm staging UI shows per-row predicted output extensions before the run.
6. **Slider extremes**: drag fully left ("Smaller file"). Confirm output is small + blocky-but-recognizable on a photo; no garbled bytes. Drag fully right ("Higher quality"). Output may be larger than the source on already-compressed JPEGs — expected.
7. **Mid-batch cancel**: pick 10+ files, hit Cancel while the progress bar is mid-way. Confirm: no half-written files in the source dir for the in-progress + un-started files; already-completed files remain.
8. **Skip-and-continue**: include a deliberately broken file (rename `random_bytes.bin` to `corrupt.jpg`). Confirm the batch continues past it, the done view lists the skipped file with its error message, and other files complete normally.
9. **Same-name collision**: re-run the same batch with the same slider. Outputs should land at `{stem}_compressed (1).{ext}`; first run's files untouched.
10. **Reveal in folder** on the done view's first row.

## Acceptance

- [ ] CI green across all three OS runners after the libwebp dep lands (commit 1) — no extra build steps needed, or any required system-dep step is added in the same commit.
- [ ] JPEG batch flow: 5+ files at midpoint slider produce visibly smaller outputs.
- [ ] PNG with alpha round-trips through the WebP path with transparency preserved.
- [ ] Mid-batch cancel leaves no half-written outputs.
- [ ] Skip-and-continue: corrupt file in the batch is listed as skipped; others succeed.
- [ ] Same-name collision routes through `unique_path`.
- [ ] Rust pure (`convert.rs`) ≥80% line coverage; orchestrator (`job.rs`) covers cancel, collision, missing input, skip-and-continue, per-format dispatch.
- [ ] Component test asserts the slider, the staging UI's predicted-output extension, the per-file size delta in the done view, the error envelope on rejection.
- [ ] **Manual smoke session before the Playwright spec** ([memory/feedback_manual_smoke_before_e2e.md]).
- [ ] One Playwright happy-path spec at `tests/e2e/image-compress.spec.ts`.
- [ ] Final DECISIONS entry pinning the per-format strategy, slider default, and EXIF behaviour.

## Open questions for the fresh session

These are things I'd weigh once development starts — flag them mid-build rather than locking down here:

- **Should the slider have a finer mapping near the top end of quality?** JPEG q=80 vs q=90 vs q=100 produces visibly different file sizes; q=20 vs q=40 less so. A non-linear curve (e.g. `q = round(1 + 99 * slider^1.5)`) would give the user more resolution in the useful range. Worth testing during smoke before locking in linear.
- **PNG-with-no-alpha** could route to JPEG instead of WebP for broader compatibility (some platforms still handle JPEG more reliably). Trade-off vs. consistency. I'd default to WebP (one rule, less surprise) but mention this in the DECISIONS entry as a deliberate choice.
- **"Compress in place" toggle?** A power-user might want to overwrite the source rather than create a `_compressed` copy. Out of scope for v1 — but if smoke surfaces "I have 500 files and don't want to clean up the originals", revisit. The `_compressed` suffix + collision handling is a safe default; an in-place toggle is a destructive operation that deserves explicit user confirmation.
- **Predict output size in the staging UI before run?** Hard to do accurately without an actual encode pass. Skip for v1; the done view's size delta is sufficient.
