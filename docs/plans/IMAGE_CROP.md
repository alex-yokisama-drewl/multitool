# Tool: Image Crop

> Ephemeral working doc. Brief first; expanded into a commit-sized plan after the user signs off on this section. Deleted when the tool ships.

## Summary
Crop a single raster image to a rectangular region selected via an overlay frame and/or numeric inputs, preserving the source format.

## Inputs
- **One** raster image, single-select picker.
- Accepted formats: the shared raster set used by the image-format converter — currently **PNG, JPEG, WebP, BMP, TIFF** (extensions: `png`, `jpg`, `jpeg`, `webp`, `bmp`, `tif`, `tiff`).
- **Single source of truth, IPC-served**:
  - New module `multitool-core/src/image/raster_format.rs` exposes a `RasterFormat` enum + per-variant metadata (`extensions()`, `default_extension()`, `display_name()`, `supports_alpha()`) and an `all()` slice.
  - `image_format_converter::TargetFormat` is migrated to re-export `RasterFormat` (or aliased). All converter call-sites keep working; no behaviour change.
  - New Tauri command `supported_raster_formats() -> Vec<RasterFormatDescriptor>` (where `RasterFormatDescriptor { id, name, extensions, supports_alpha }`) lives alongside the crop shell module — it's a shared system query, not tool-specific, so it registers from `src-tauri/src/lib.rs` (or a small `system` module if we end up with more than one).
  - New TS wrapper `src/lib/imageFormats.ts` calls the command once, memoizes the result, and exposes `getRasterFormats()` + `rasterImageExtensions()`. The crop picker uses the latter for its dialog filter.
  - In-scope micro-refactor: the image-format converter's **output-format dropdown** stops hard-coding the list and reads from `getRasterFormats()` — kills the second sync surface in one stroke. (The converter's **input-side** picker accepts a broader decode-only set — SVG/GIF/ICO/etc. — and is out of scope for this PR; that sync surface stays as-is.)
- EXIF orientation is honored on decode via the shared `decode_oriented` helper, so the user crops against the visually-correct image. The output is written upright (no orientation tag).

## Options
| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `rect.x` | i32 (pixels) | 0 | Top-left X of the crop region in source-image pixel coords (post-orientation). May be negative; backend clamps. |
| `rect.y` | i32 (pixels) | 0 | Top-left Y. May be negative; backend clamps. |
| `rect.width` | u32 (pixels) | source width | Backend forces 0 → 1; backend clamps so `x + width ≤ source width` (after also clamping `x ≥ 0`). |
| `rect.height` | u32 (pixels) | source height | Backend forces 0 → 1; backend clamps so `y + height ≤ source height`. |

No alpha-handling knob (output format equals input format, so the alpha-capability question doesn't change). No JPEG-quality knob: when the source is JPEG, the encoder uses a fixed `quality = 90`.

### Frame UX (frontend only — not part of the IPC payload)
- Frame starts at the full image (`x=0`, `y=0`, `width=W`, `height=H`).
- Draggable: 4 corners (2-axis resize), 4 sides (1-axis resize), body (translate; size unchanged).
- Numeric inputs and the visual frame are bidirectionally bound — editing one updates the other immediately.
- "Proportional resize" checkbox: on toggle-on, captures the current `width / height` ratio. While enabled, any resize op (corner, side, or numeric W/H edit) preserves the locked ratio; body translation is unaffected. Toggling off releases the lock. Toggling off-then-on re-captures the then-current ratio.
- Frame is constrained to the image bounds; drags clamp at the edges. Minimum size 1×1 pixel.

## Output
- **Location:** same directory as the input.
- **Naming:** `{stem}_cropped.{ext}` (extension preserved from input, lowercase canonical: `jpeg`→`jpg`, `tiff`→`tif`).
- **Duplicate handling:** `multitool_core::fs::unique_path` — appends ` (1)`, ` (2)`, … per [ARCHITECTURE §3.3](../ARCHITECTURE.md#33-file-io-conventions).

## UX flow
1. Dashboard → "Image Crop" tile.
2. Pick a file → image renders with the frame fully covering it; numeric inputs populated.
3. User adjusts frame (drag) and/or numeric inputs; "proportional resize" available.
4. Click "Crop" → spinner → success state with "Open output folder" and "Crop another" actions.
5. Error envelope renders inline on failure, preserving the picked file so the user can retry without re-picking.

No long-running progress — crop on a single image is fast enough that a determinate progress bar would just flash. The pattern still threads a `CancellationToken` through `convert` (constant requirement from CLAUDE.md / ARCHITECTURE §3.2); the UI surfaces a Cancel button during the in-flight `cropping` state to abort if something pathological happens (giant TIFF, slow disk).

## Edge cases

Backend clamping policy: silently fix the cases that have a well-defined intent ("user almost certainly meant a rect inside the image"); error on the cases where nothing meaningful can be inferred. The frontend also clamps as UX polish (no jittery frame escapes the image); the backend re-clamps as the source of truth so `crop()` is a safe primitive for future modules to reuse.

- **Zero-size rect** (`width == 0` or `height == 0`) — backend silently forces the offending dimension to 1. No warning.
- **Rect partially outside** image bounds — backend silently clamps to the intersection with the image (so `(x=-5, y=0, w=20, h=20)` against a 10×10 image becomes `(0, 0, 10, 10)`). No warning.
- **Rect entirely outside** (no intersection: `x ≥ width` or `y ≥ height` or `x + w ≤ 0` or `y + h ≤ 0`) — rejected with `ProcessingFailed { detail: "crop rectangle does not intersect the image" }`. No sensible clamp; the caller is broken.
- **Source is a renamed file** (e.g. PNG bytes named `.jpg`) — the decoder sniffs bytes (`with_guessed_format`); on success, we write the output using the **source extension** because that's what "preserve format" means to the user. Re-encode targets the extension's format, so a PNG-bytes-as-`.jpg` re-encodes as JPEG. Same convention as the image-format converter on input-decode; least-surprising for a "crop" tool.
- **Multi-frame TIFF** — rejected with `UnsupportedFormat { detail: "multi-frame TIFF is not supported; only single-frame TIFFs can be cropped while preserving the source format" }`. Detection happens early (before decode) by attempting to read the frame count; single-frame falls through to the normal path. (Animated GIF isn't in the supported set, so doesn't apply here.)
- **Very large image** (e.g. 12000×9000 PNG) — decode + encode dominates; no chunking. Acceptable for v1; revisit if smoke surfaces a real-time problem.
- **EXIF orientation = 6/8 (rotated)** — coords are in the post-orientation pixel space (what the user sees). Output is upright pixels with no EXIF orientation tag.

Rect type at the IPC boundary uses **signed** ints (`i32` x/y, `u32` w/h, or all-signed) so the partial-outside case can carry negative `x`/`y` end-to-end without overflow before the backend clamps. Frontend wrapper validates non-negative `w`/`h` before invoking.

## Acceptance
- [ ] Picker accepts `png|jpg|jpeg|webp|bmp|tif|tiff`; no other extensions in the dialog filter. List comes from the `supported_raster_formats` IPC (no hardcoded extension array on the TS side).
- [ ] Shared `RasterFormat` module exists in `multitool-core`; `image_format_converter::TargetFormat` re-exports / aliases it; image-crop consumes it.
- [ ] `supported_raster_formats` Tauri command + `src/lib/imageFormats.ts` wrapper with memoized fetch shipped.
- [ ] Image-format converter's output dropdown migrated to consume `getRasterFormats()` (eliminates the second hardcoded list).
- [ ] Backend `crop()` clamps zero-size to 1px and partial-outside to the intersection; errors only on no-intersection and multi-frame TIFF.
- [ ] Frame defaults to the full image on first render after pick.
- [ ] Dragging corners/sides resizes the frame; dragging the body translates it; clamped to image bounds; min 1×1.
- [ ] Numeric inputs and frame are bidirectionally bound (no perceptible lag).
- [ ] Proportional-resize checkbox locks the W:H ratio at the moment it's toggled on; resize ops respect the lock; body translation is unaffected.
- [ ] Output format equals input format (PNG→PNG, JPEG→JPEG @ q=90, WebP→lossless WebP, BMP→BMP, TIFF→TIFF).
- [ ] Output file is `{stem}_cropped.{ext}` in the source dir; duplicates collide via `unique_path`.
- [ ] Rust unit coverage ≥ 80 % on `convert.rs`; orchestrator covers happy path + cancellation + missing input + collision + invalid rect.
- [ ] Vitest covers IPC wrapper (invoke, abort, error envelope) and component (defaults, options forwarded, error renders, cancel aborts).
- [ ] Manual smoke session done in `pnpm tauri dev` before the Playwright spec is written (per `feedback_manual_smoke_before_e2e`).
- [ ] Playwright happy-path: dashboard → tile → pick → crop → success visible.
- [ ] Pre-PR checklist green (fmt, clippy, cargo test, lint, typecheck, vitest, `tauri build --no-bundle`, e2e).

## Plan

Commit-sized tasks. Update each row in-place after the commit lands (status `pending` → `done`, paste SHA, add a one-line gotcha if anything surprising). Per [feedback_update_working_doc_per_commit](../../.claude/projects/...) — fresh sessions should be able to read this and pick up exactly where the last one stopped.

| # | Commit (conv. commits) | Scope | Status | SHA | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | `refactor(image): extract RasterFormat into shared module` | New `multitool-core/src/image/raster_format.rs` — `RasterFormat` enum + metadata methods (`extensions`, `default_extension`, `display_name`, `supports_alpha`, `all()`, `image_format()`). `image_format_converter::TargetFormat` becomes `pub type TargetFormat = RasterFormat;` re-export to avoid churning every call site. Existing converter tests pass unchanged. | done | `83c83f1` | `image.rs` → `image/mod.rs` (dir module). Old `extension()` renamed `default_extension()`; `image_format()` made `pub`. `RasterFormatDescriptor` already added here (not deferred to #2) — it's a pure serde type. 228 core tests green. |
| 2 | `feat(system): supported_raster_formats IPC command` | `RasterFormatDescriptor` (Serialize) in the new shared module + `#[tauri::command] fn supported_raster_formats()` in `src-tauri`. Registered from `lib.rs`. Pure descriptor-builder test in `multitool-core`. | done | `0744d58` | Command lives in new `src-tauri/src/system.rs` (sibling to `asset_scope`), registered in `tools/mod.rs::register_commands`. Descriptor-builder logic covered by `descriptor_list_is_built_for_every_format_with_unique_ids` in core. |
| 3 | `feat(lib): imageFormats wrapper with memoized fetch` | `src/lib/imageFormats.ts` — `getRasterFormats()` (cached) + `rasterImageExtensions()`. Vitest mocks `invoke` and asserts: descriptor round-trip, memoization (one call across N reads), extension flattening. | done | `d5111d1` | Memo caches the in-flight promise (dedupes concurrent callers) but clears on rejection so a transient IPC failure isn't sticky. Added `__resetRasterFormatsCache()` test-only export since the memo is module-scoped and Vitest shares module state across cases. |
| 4 | `refactor(image-format-converter): consume IPC for output dropdown` | Migrate the converter's frontend output-format dropdown from its hardcoded list to `getRasterFormats()`. Update converter component tests to mock `getRasterFormats()`. No UX change. | done | `99cb4b6` | Also derived the alpha-handling gate from `supports_alpha` (dropped the hardcoded `ALPHA_LESS_TARGETS` set). WebP's "(lossless)" hint kept as a 1-entry `FORMAT_NOTE` map (UI note, not a format list). **Gotcha:** converter already had an e2e spec → had to add `tests/e2e/mocks/imageFormats.ts` + vite alias or the converter e2e would hit real `invoke`. That mock is reused by the crop e2e (#11). |
| 5 | `feat(image-crop): rust crop convert + multi-frame TIFF guard` | `multitool-core/src/tools/image_crop/{mod.rs,convert.rs}`. `Rect { x: i32, y: i32, width: u32, height: u32 }`, `Opts { rect }`. `crop_one(source_ext, bytes, opts) -> AppResult<EncodedFile>` — early multi-frame-TIFF detection, decode via `decode_oriented`, clamp (zero→1, partial→intersection, no-intersection→error), `image::DynamicImage::crop_imm`, re-encode to source extension's format (JPEG q=90). Unit tests: 5-format round-trip, EXIF orientation, clamping matrix (zero W/H, negative XY, partial OOB, full OOB), multi-frame TIFF rejection. Target ≥ 80 % line cov. | done | `e7147ae` | Naming differs from the brief sketch: no `Opts` wrapper — `crop_one(source_ext, bytes, &CropRect)` returns `Vec<u8>` (no warnings to carry, so no `EncodedFile`). Clamp is a standalone `CropRect::clamp_to(w,h) -> Option<PixelRect>` primitive. Multi-frame TIFF detection is a hand-rolled classic-TIFF IFD walker (`image`/`tiff` don't expose a frame count); BigTIFF/malformed → `None` → falls through to decoder. 96% line cov on convert.rs. |
| 6 | `feat(image-crop): rust job orchestrator + tauri command` | `multitool-core/src/tools/image_crop/job.rs` — `run_job(path, opts, cancel, on_progress) -> AppResult<JobResult>`: read input, call `crop_one`, write to `{stem}_cropped.{ext}` via `unique_path`. `src-tauri/src/tools/image_crop/mod.rs` shim using `run_streaming_job`. Registered in `src-tauri/src/tools/mod.rs`. Orchestrator tests: happy path, cancellation (returns `Cancelled`, leaves no partial file), missing input, `unique_path` collision, invalid-rect propagation, multi-frame TIFF propagation. | done | `52d3544` | Command name `crop_image`; arg is `opts: CropRect` (no wrapper). `JobResult { output, duration_ms }` — no `warnings` field (crop has none). 8 orchestrator tests green; shell build confirms `generate_handler!` registration. |
| 7 | `feat(image-crop): TS IPC wrapper` | `src/lib/tools/imageCrop.ts` + `imageCrop.test.ts`. Pattern: JobId via `crypto.randomUUID()`, `invoke<JobResult>("convert_image_crop", { jobId, path, opts })`, `AbortSignal` → `cancel_job`, `try/finally` listener cleanup. Tests: invokes the right command, error envelope round-trip, AbortSignal aborts, no listener leak on error. | pending | | |
| 8 | `feat(image-crop): React component with frame editor` | `src/tools/image-crop/{index.ts, ImageCrop.tsx, types.ts, ImageCrop.test.tsx}`. State machine `idle → picked → cropping → done | error`. New `pickRasterImage()` in `src/lib/system.ts` (single-select, awaits `rasterImageExtensions()` for the filter). `<img>` rendered via `imageAssetUrl(path)` (requires `allowMediaPreview` for the picked path). Frame overlay: corners/sides/body drag handlers + numeric inputs bidirectionally bound, proportional-resize checkbox captures ratio at toggle-on. Vitest: defaults render, options forwarded, error envelope renders, cancel aborts. | pending | | |
| 9 | `chore(image-crop): register tool + dashboard tile` | Add import + entry in `src/tools/registry.ts`; update `src/app/Dashboard.test.tsx` to assert the new tile. Only edits to shared frontend files. | pending | | |
| 10 | — **manual smoke pause** — | Per [feedback_manual_smoke_before_e2e](../../.claude/projects/...) — stop here and ask the user to smoke-test in `pnpm tauri dev` before writing the e2e spec. Scenarios to eyeball listed in the ask. Bugs surfaced here become their own commits before #11. | pending | | |
| 11 | `test(image-crop): playwright happy-path e2e` | `tests/e2e/image-crop.spec.ts` — dashboard → tile → pick (mocked) → crop → success state visible. New e2e mock under `tests/e2e/mocks/` if `imageCrop.ts` needs one. | pending | | |
| 12 | `chore(image-crop): cleanup + ship` | Remove "Image crop." from `docs/plans/BACKLOG.md`. Delete this working doc. Add DECISIONS entry if anything load-bearing emerged (likely the IPC-served format set pattern). Run full pre-PR checklist. | pending | | |

