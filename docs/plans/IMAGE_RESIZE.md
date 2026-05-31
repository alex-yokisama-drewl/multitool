# Tool: Image Resize

> Ephemeral working doc. Brief first; expanded into a commit-sized plan after the user signs off on this section. Deleted when the tool ships.

## Summary
Resize a single raster image to a user-specified width and height, preserving the source format. Aspect ratio can be locked (W and H move together); when unlocked, an explicit fit policy (stretch / contain / cover) governs how the source maps into a target canvas whose aspect differs from the source's.

## Inputs
- **One** raster image, single-select picker (same `pickRasterImage()` the crop tool uses).
- Accepted formats: the shared `RasterFormat` set — **PNG, JPEG, WebP, BMP, TIFF**. Same "output format == input format" contract as crop, so the picker filters down to the encodable set; GIF / SVG / HEIC are out of scope.
- EXIF orientation honored on decode via `decode_oriented`; output written upright (no orientation tag).

## Options
| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `width` | `u32` (pixels) | source width | Clamped `[1, MAX_DIM]` on the backend. UI also clamps. |
| `height` | `u32` (pixels) | source height | Clamped `[1, MAX_DIM]` on the backend. UI also clamps. |
| `fit` | `Stretch \| Contain \| Cover` | `Contain` | Ignored when the source-and-target aspect ratios match (the three modes collapse to "scale"). Sent regardless — backend decides whether it matters. |
| `background` | `White \| Black \| Transparent` | `Transparent` if source supports alpha else `White` | **Only used by `Contain`** to fill letterbox bars. Validated against source format: `Transparent` requires `source_ext.supports_alpha()` (PNG / WebP / TIFF); on JPEG / BMP it errors as `UnsupportedFormat`. Same gating shape as the converter's `AlphaHandling::Preserve`. |

`MAX_DIM` = **16384** per side. Above this an encoder buffer balloons past ≈ 1 GB (16384² × 4 bytes), past the "interactive desktop tool" envelope. Silently clamped, matching the `jpeg_quality` / `SVG_PX_MAX` clamp policy elsewhere.

No `jpeg_quality` knob: source = JPEG re-encodes at the same fixed `quality = 90` crop uses. Quality control is the format converter's job. No resize-filter knob: **Lanczos3** is the only choice (the `image` crate's standard high-quality default).

### UI affordances (frontend only — not part of the IPC payload)
- **W / H numeric inputs**: free-text editing, commit on blur / Enter (same `NumberField` pattern as crop's [ImageCrop.tsx:257-291](../../src/tools/image-crop/ImageCrop.tsx#L257-L291)).
- **Lock checkbox** ("Lock aspect ratio"): on toggle-on, captures the current `width / height` ratio at that instant — same semantics as crop's lock ([ImageCrop.tsx:78-82](../../src/tools/image-crop/ImageCrop.tsx#L78-L82)). While enabled, editing W auto-updates H (and vice versa) to preserve the locked ratio. Toggling off releases the lock; off-then-on re-captures.
- **Reset button**: restores typed W and H to the source's natural dimensions. Does **not** toggle the lock or change the fit mode (those are UI preferences, not values).
- **Fit radio group** (Stretch / Contain / Cover): only visible when lock is OFF — locked resize is by definition aspect-preserving, so the three modes collapse.
- **Background radio group** (White / Black / Transparent): only visible when fit is `Contain` AND lock is OFF. `Transparent` is conditionally hidden when the source format can't carry alpha (JPEG / BMP) — same conditional-render pattern as the converter's `showAlphaHandling` derived from `RasterFormatDescriptor.supports_alpha`.

Resulting conditional-UI tree:

```
[W] [H] [Reset] [☑ Lock aspect ratio]
└─ Lock OFF
   └─ Fit: ( ) Stretch  ( ) Contain  ( ) Cover
      └─ if Contain:
         Background: ( ) White  ( ) Black  [( ) Transparent — only if source has alpha]
```

## Output
- **Location:** same directory as the input.
- **Naming:** `{stem}_resized.{ext}` (extension preserved from input, lowercase canonical: `jpeg`→`jpg`, `tiff`→`tif`). Matches crop's `_cropped` convention.
- **Duplicate handling:** `multitool_core::fs::unique_path` per [ARCHITECTURE §3.3](../ARCHITECTURE.md#33-file-io-conventions).

## UX flow
1. Dashboard → "Resize" tile.
2. Pick a file → image preview renders; W/H inputs populated with the source's natural dims; lock defaults to ON; fit + background hidden (lock ON).
3. User adjusts W / H (with or without lock), picks fit mode if unlocked, picks background if Contain.
4. Click "Resize" → spinner → success state with "Open output folder" and "Resize another" actions.
5. Error envelope renders inline on failure, preserving the picked file so the user can retry without re-picking.

No long-running progress bar — resize on a single image is fast enough that a determinate bar would just flash. Cancellation still wired (constant requirement per CLAUDE.md / ARCHITECTURE §3.2); UI surfaces a Cancel button during the in-flight `resizing` state for pathological-size inputs (a 16k × 16k re-encode is in the seconds, not milliseconds).

## Algorithm (per fit mode)

Let `(Ws, Hs)` = source dims, `(W, H)` = target dims. All resizes use `Lanczos3`.

- **Stretch**: `image::DynamicImage::resize_exact(W, H, Lanczos3)`. Distortion possible — by user choice.
- **Contain**:
  1. `scale = min(W / Ws, H / Hs)`
  2. Resize source to `(round(Ws · scale), round(Hs · scale))` with `resize_exact`.
  3. Allocate a `W × H` canvas filled with the chosen background color (RGB for white/black, RGBA with α=0 for transparent).
  4. Composite the resized source into the canvas, centered (`((W − w′)/2, (H − h′)/2)` rounded toward 0).
- **Cover**:
  1. `scale = max(W / Ws, H / Hs)`
  2. Resize source to `(round(Ws · scale), round(Hs · scale))` with `resize_exact`.
  3. Center-crop to `W × H` (`crop_imm` at the centered offset).

No anchor knob for Cover — center-crop is the default everywhere this mode exists (Photoshop's "Fill", CSS `object-fit: cover` minus `object-position`); add it back if a smoke session calls for it.

When `(Ws, Hs)` and `(W, H)` are exactly proportional (e.g. lock-mode output), all three fit modes produce pixel-identical results. The backend doesn't special-case this — it's a natural fall-out of the math.

## Edge cases

Same clamp-then-error philosophy as crop: silently fix recoverable inputs; reject only the ones with no defensible interpretation.

- **Zero dims** (`width == 0` or `height == 0`) — silently forced to `1`. No warning.
- **Over-large dims** (`> MAX_DIM`) — silently clamped to `MAX_DIM`. No warning. Same policy as `jpeg_quality` / `SVG_PX_MAX`.
- **Transparent background on an alpha-less source** (JPEG / BMP) — rejected with `UnsupportedFormat { detail: "target format does not support alpha; choose a white or black background" }`. Surfaces in the error envelope; UI's conditional render shouldn't let this slip through, but the backend re-validates.
- **Renamed file** (PNG bytes named `.jpg`) — decoder sniffs bytes (`decode_oriented`); output is encoded against the **source extension** ("preserve format" = preserve the extension). Same convention as crop.
- **Multi-frame TIFF** — rejected with `UnsupportedFormat { detail: "multi-frame TIFF is not supported; only single-frame TIFFs can be resized while preserving the source format" }`. Detection happens before decode by walking the IFD chain — reuse / lift crop's `tiff_frame_count` if it's still inline there ([convert.rs:169-210](../../src-tauri/multitool-core/src/tools/image_crop/convert.rs#L169-L210)).
- **Output dims == source dims** — pass-through encode (no resize op), so the JPEG path still re-encodes at `quality = 90`. Acceptable: this is a resize tool; quality preservation is not part of its contract.
- **EXIF orientation = 6/8 (rotated)** — W/H apply to the post-orientation pixel space (what the user sees). Output written upright with no orientation tag. Same as crop.
- **Very large input** (e.g. a 12000 × 9000 PNG resized to 800 × 600) — single-pass decode → resize → encode dominates; no chunking. Cancellation token is checked between stages so a stuck `resize_exact` on a pathological input doesn't block forever.

## Shared-surface reuse

- `RasterFormat` + `RasterFormatDescriptor` + `getRasterFormats()` — for the picker filter and the conditional `Transparent` background option.
- `decode_oriented` — EXIF-aware decode.
- `JPEG_QUALITY = 90` — lift from `image_crop::convert` into `multitool_core::image` (or duplicate the constant if the lift feels premature; revisit if a third tool needs it).
- `tiff_frame_count` — multi-frame TIFF guard, currently a private helper in `image_crop::convert`. Lift to `multitool_core::image` and have both tools consume it.
- `unique_path` — output-name collision resolution.
- `pickRasterImage()` — single-select picker, already in `src/lib/system.ts`.
- `imageAssetUrl` / `allowMediaPreview` — `<img>` rendering for the picked file.
- `run_streaming_job` — Tauri command shim.
- `NumberField` pattern from `ImageCrop.tsx` — keep inline copy in the resize component; promote to `src/components/` only if a third tool needs it (per [ADDING_A_TOOL.md → Shared components](../ADDING_A_TOOL.md#shared-surfaces)).

## Acceptance
- [ ] Picker accepts `png|jpg|jpeg|webp|bmp|tif|tiff`; list comes from `rasterImageExtensions()`.
- [ ] On pick, W/H populate with the source's natural (post-orientation) dims; lock starts ON.
- [ ] Lock checkbox captures the current W:H ratio at toggle-on and keeps W and H in sync as the user edits either.
- [ ] Reset button restores W and H to the source's natural dims; does not touch lock state or fit mode.
- [ ] Fit radio group is hidden when lock is ON; shown when OFF. Background radio group is hidden unless fit == Contain. Transparent option is hidden when the source format doesn't support alpha.
- [ ] Backend clamps zero dims to 1px and over-MAX_DIM dims to MAX_DIM; errors only on `Transparent` background with an alpha-less source format, and on multi-frame TIFF.
- [ ] Stretch, Contain, and Cover produce the documented dims (Stretch: target W×H; Contain: target W×H with centered letterbox; Cover: target W×H from a center-crop of the scaled source).
- [ ] Contain background fill produces the expected color on the bars (corner-pixel sample): white, black, or α=0 transparent.
- [ ] Output format equals input format (PNG→PNG, JPEG→JPEG @ q=90, WebP→lossless WebP, BMP→BMP, TIFF→TIFF).
- [ ] Output file is `{stem}_resized.{ext}` in the source dir; duplicates collide via `unique_path`.
- [ ] EXIF-oriented source: resize operates in post-orientation space; output upright with no orientation tag (round-trip a `rotated.jpg` fixture).
- [ ] Renamed-file convention: PNG bytes named `.jpg` re-encode as JPEG.
- [ ] Multi-frame TIFF rejected with `UnsupportedFormat`.
- [ ] Rust unit coverage ≥ 80 % on `convert.rs`; orchestrator covers happy path + cancellation + missing input + collision + invalid-options propagation.
- [ ] Vitest covers IPC wrapper (invoke, abort, error envelope) and component (defaults, lock semantics, reset, conditional UI render, options forwarded, error renders, cancel aborts).
- [ ] Manual smoke session done in `pnpm tauri dev` before the Playwright spec is written (per [feedback_manual_smoke_before_e2e](../../.claude/projects/-home-yokisama-Projects-multitool/memory/feedback_manual_smoke_before_e2e.md)).
- [ ] Playwright happy-path: dashboard → tile → pick → resize → success visible.
- [ ] Pre-PR checklist green (fmt, clippy, cargo test, lint, typecheck, vitest, `tauri build --no-bundle`, e2e).

## Plan

Commit-sized tasks. Update each row in-place after the commit lands (status `pending` → `done`, paste SHA, add a one-line gotcha if anything surprising). Per [feedback_update_working_doc_per_commit](../../.claude/projects/-home-yokisama-Projects-multitool/memory/feedback_update_working_doc_per_commit.md) — fresh sessions should be able to read this and pick up exactly where the last one stopped.

| # | Commit (conv. commits) | Scope | Status | SHA | Notes |
| --- | --- | --- | --- | --- | --- |
| 0 | `docs(image-resize): brief + plan working doc` | This file. All rows below `pending`. | done | `767014b` | |
| 1 | `refactor(image): lift JPEG_REENCODE_QUALITY + tiff_frame_count to shared module` | Lift the JPEG re-encode quality constant from `image_crop::convert` into `multitool_core::image` as `pub const JPEG_REENCODE_QUALITY: u8 = 90` (separate from the converter's user-controlled `jpeg_quality`). Lift `tiff_frame_count` + its `multi_ifd_tiff` test helper into a new `multitool_core::image::tiff` module — `tiff_frame_count` becomes `pub fn`, helper stays under `#[cfg(test)] pub(crate)`. Crop's `convert.rs` updates imports; crop tests that hit `tiff_frame_count` directly stay green; orchestrator tests keep their own `multi_ifd_tiff` helper unchanged. No behavior change; 228+ core tests stay green. | done | `fb57122` | 309 multitool-core tests passing post-lift (count grew from 228 since the plan was drafted — other tools have added tests). Crop's `multi_ifd_tiff` and direct `tiff_frame_count` tests both removed (they live in `image::tiff` now); only the end-to-end `multi_frame_tiff_is_rejected` test remains in crop, using the shared helper via `use crate::image::tiff::multi_ifd_tiff;`. |
| 2 | `feat(image-resize): rust convert with fit modes + background fill` | `multitool-core/src/tools/image_resize/{mod.rs,convert.rs}`. Types: `FitMode { Stretch, Contain, Cover }`, `Background { White, Black, Transparent }`, `Opts { width, height, fit, background }`. `resize_one(source_ext, bytes, opts) -> AppResult<Vec<u8>>`. Pipeline: `RasterFormat::from_extension` → multi-frame TIFF guard via the lifted `tiff_frame_count` → clamp dims to `[1, MAX_DIM]` → `decode_oriented` → validate `Transparent` vs `target.supports_alpha()` → dispatch on `FitMode` → re-encode via local `encode_to` using the lifted `JPEG_REENCODE_QUALITY`. Contain math: `scale = min(W/Ws, H/Hs)`; resize source with `resize_exact(_, _, Lanczos3)`; allocate canvas (RGBA + α=0 for Transparent, RGBA + bg for White/Black then flatten to RGB after overlay); center-place via `image::imageops::overlay`. Cover math: `scale = max(...)`; resize; center-crop via `crop_imm`. Stretch: direct `resize_exact`. Unit tests covering: 5-format round-trips, Stretch up/down/aspect-mismatched, Contain dims + corner-pixel bg color sample (white/black/α=0), Contain Transparent on JPEG/BMP → `UnsupportedFormat`, Cover dims + center-crop sample, lock-equivalent (target aspect == source aspect) parity across modes, multi-frame TIFF rejection, renamed-file (PNG-as-JPG → JPEG out), EXIF orientation honored, zero dims → 1, over-MAX_DIM clamp, garbage bytes → decode error. Target ≥ 80 % line cov. | done | `1b7b60d` | Coverage 97.63% lines / 97.64% regions on `convert.rs` (well above the 80% target). Two design refinements: (a) Transparent-on-alpha-less-target only errors when `fit==Contain` — Stretch/Cover never read `background`, so a stale Transparent choice in those modes is silently ignored (matches the UI's hide-bg-outside-Contain rule). (b) The Cover assertion against `red.png` discovered the fixture is uniform `(200, 30, 30)`, not pure red — assertion updated to match the actual source color. |
| 3 | `feat(image-resize): rust job orchestrator + tauri command` | `multitool-core/src/tools/image_resize/job.rs` — `run_job(source, opts, cancel, on_progress) -> AppResult<JobResult>`: cancel checkpoints before/after read, `resize_one`, write to `{stem}_resized.{ext}` via `unique_path`. `Progress::Started { source }` (single variant, mirroring crop). `src-tauri/src/tools/image_resize/mod.rs` — `#[tauri::command] async fn resize_image(...)` using `run_streaming_job`. Register in `src-tauri/src/tools/mod.rs` + `generate_handler!`. Orchestrator tests: happy path writes `{stem}_resized.{ext}`, missing input → `FileNotFound`, multi-frame TIFF propagates + writes nothing, Transparent-on-JPEG propagates + writes nothing, cancel pre-read + post-read both → `Cancelled` with no writes, `unique_path` collision lands at ` (1)`, on_progress error aborts before read. | pending | | |
| 4 | `feat(image-resize): TS IPC wrapper` | `src/lib/tools/imageResize.ts` + `imageResize.test.ts`. Mirrors `imageCrop.ts`: `FitMode`, `Background`, `Opts`, `Progress` (single `started` variant), `JobResult { output, duration_ms }`. `resizeImage(path, opts, hooks)` → `runJob<{path, opts}, Progress, JobResult>("resize_image", …)`. Tests mirror `imageCrop.test.ts`: invokes right command, Started filtered by JobId, listener unsubscribed on success + error, AbortSignal → `cancel_job`, pre-aborted signal throws without invoking. | pending | | |
| 5 | `feat(image-resize): React component with lock + fit + bg controls` | `src/tools/image-resize/{index.ts, ImageResize.tsx, types.ts, ImageResize.test.tsx}`. State machine `idle → picked → resizing → done | error`. Geometry state (`dims`, `width`, `height`, `lockedRatio`, `fit`, `background`) lives outside ViewState (survives picked↔resizing transitions). On pick: load image, capture natural dims, populate W/H, lock defaults ON (ratio = Ws/Hs). Inline `NumberField` copy (same pattern as crop). Reset button restores W/H to dims; preserves lock + fit. Lock toggle captures current W/H ratio; W edits with lock on auto-update H, and vice versa. Conditional render: fit radio group when lock OFF; bg radio group when lock OFF AND fit == Contain; `Transparent` option only when source ext supports alpha (consult `getRasterFormats()` for the picked file's extension). Vitest: defaults render, lock semantics (toggle captures ratio, W edits update H), reset restores dims, conditional UI render (lock off shows fit; contain shows bg; jpeg hides Transparent), options forwarded to wrapper, error envelope renders, cancel aborts the signal. | pending | | |
| 6 | `chore(image-resize): register tool + dashboard tile` | Add import + entry in `src/tools/registry.ts`; tile color `emerald` (distinct from sky/violet/amber used by the other image tools). Update `src/app/Dashboard.test.tsx` to assert the new tile. Only edits to shared frontend files. | pending | | |
| 7 | — **manual smoke pause** — | Per [feedback_manual_smoke_before_e2e](../../.claude/projects/-home-yokisama-Projects-multitool/memory/feedback_manual_smoke_before_e2e.md) — stop and ask the user to smoke-test in `pnpm tauri dev` before the e2e spec. Scenarios to eyeball: golden flow on a PNG and a JPEG; lock semantics (toggle off + type W/H independently, toggle back on); Stretch / Contain / Cover visually distinguishable on a clearly-non-aspect-matching target (e.g. 800×800 from a 100×50 source); Contain with each bg option (white/black/transparent — verify the Transparent option vanishes on JPEG); Reset; mid-job cancel on a giant input; error envelope on a deliberately-renamed garbage file. Bugs surfaced here become their own commits before #8. | pending | | |
| 8 | `test(image-resize): playwright happy-path e2e` | `tests/e2e/image-resize.spec.ts` — dashboard → tile → pick (mocked) → resize → success state visible. New e2e mock under `tests/e2e/mocks/imageResize.ts` modeled after the crop mock. | pending | | |
| 9 | `chore(image-resize): cleanup + ship` | Remove "Image resize." from `docs/plans/BACKLOG.md`. Delete this working doc. Add DECISIONS entry only if anything load-bearing emerged. Run full pre-PR checklist (fmt, clippy, cargo test, lint, typecheck, vitest, `tauri build --no-bundle`, e2e). | pending | | |
