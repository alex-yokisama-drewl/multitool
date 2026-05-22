# Tool: Image Format Converter

> Ephemeral assignment brief. Lives here while the tool is being built; deleted once shipped. Tool ID: `image-format-converter`. Rust module: `image_format_converter`.

## Summary

Convert one or more images from one raster format to another, in batch, fully offline. No raster↔vector conversions — this is a re-encoder, not a tracer.

## Scope: which formats

Outputs are raster only (no tracing). Inputs are broader: anything decodable, including SVG (rasterized via `resvg`).

| Format | Input | Output | Notes |
| --- | --- | --- | --- |
| PNG    | ✅ | ✅ | Lossless. 8-bit and 16-bit decode; encode is 8-bit. |
| JPEG   | ✅ | ✅ | Lossy. No alpha channel — see [Alpha handling](#alpha-handling). |
| WebP   | ✅ | ✅ | **Output is lossless only** (the `image` 0.25 encoder doesn't expose a lossy mode; lossy WebP would need libwebp). Alpha supported. Decode handles both lossy and lossless WebP inputs. |
| BMP    | ✅ | ✅ | Uncompressed. No alpha in encoder (see alpha handling). |
| TIFF   | ✅ | ✅ | Lossless. Single-image; multi-page TIFFs decode the first image only. |
| GIF    | ✅ | ❌ | Input only. Animated GIFs decode the **first frame** with a UI warning; output GIF deferred (single-palette encoder constraints aren't worth handling now — animated decomposition could be a separate tool later). |
| ICO    | ✅ | ❌ | Input only. Containers — pick the largest embedded image. |
| TGA    | ✅ | ❌ | Free via the `image` crate's default features. |
| PNM    | ✅ | ❌ | PBM/PGM/PPM family. Free via `image`. |
| QOI    | ✅ | ❌ | Free via `image`. |
| **SVG**    | ✅ | ❌ | **Vector input, rasterized via `resvg`**. The user picks the raster size — see [SVG rasterization](#svg-rasterization). |

**Out of scope (record as follow-ups; do not implement in this tool):**

- SVG output ("png → svg" tracing) — not a re-encode; that's a different product entirely.
- HEIC / HEIF — no pure-Rust decoder in the `image` ecosystem.
- AVIF — encoder needs `ravif` + extra build setup; revisit if requested.
- PDF input — `pdf-to-images` already covers PDF rasterization.

## Inputs

- One or more image files via the file picker (multi-select). Supported input extensions: `.png .jpg .jpeg .webp .bmp .tif .tiff .gif .ico .tga .pbm .pgm .ppm .pnm .qoi .svg`.
- The picker filter is advisory; the Rust side re-validates by decoding (mirrors the [`asset_scope.rs` pattern](../../src-tauri/src/asset_scope.rs) where extension filters are not trusted). SVG detection is by extension (the decoder is a different code path) — non-SVG bytes with an `.svg` extension fall to `UnsupportedFormat`.
- Staging list UX (lighter than Images → PDF — order doesn't matter so there is no reorder):
  - Each picked file appears as a row with filename + size.
  - "Add more" appends to the list; supports inputs from multiple directories in one job.
  - Per-row remove control.
  - Nothing is written until the user clicks "Convert".

## Options

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `target_format` | enum (`png` \| `jpeg` \| `webp` \| `bmp` \| `tiff`) | `png` | Single dropdown; applies to all staged inputs. |
| `jpeg_quality` | `u8` ∈ [1, 100] | `85` | Shown only when `target_format == jpeg`. |
| `alpha_handling` | enum (`preserve` \| `flatten-white` \| `flatten-black`) | `flatten-white` | Applied **only** when the target format lacks alpha (JPEG, BMP); silently ignored otherwise. `preserve` for an alpha-less target means: skip the file with a `ProcessingFailed` reported in the [batch error list](#batch-error-policy) — explicit refusal beats accidental flattening. |
| `svg_raster_size` | enum (`natural` \| `longest-edge-px:N`) | `longest-edge-px:1024` | Used only for SVG inputs. `natural` honors the SVG's `width`/`height`; `longest-edge-px:N` scales so the longest side is exactly N pixels (1 ≤ N ≤ 8192, clamped). UI hides this option when no staged input is `.svg`. |

Backend clamps quality and px values to their declared ranges defensively (UI clamps too); mirrors the DPI clamp pattern in [`pdf_to_images/convert.rs`](../../src-tauri/multitool-core/src/tools/pdf_to_images/convert.rs).

### Batch error policy

Skip + continue. Per-file decode/encode failures don't abort the job — they're recorded and surfaced at completion as an error list (`{ path, error }` per failure). The UI shows: `N converted, M skipped` with an expandable list of skipped files and reasons. Cancellation and unrecoverable orchestrator errors (e.g., couldn't read input bytes off disk) still abort the job per the existing `AppError::Cancelled` / `AppError::FileNotFound` shape.

## Output

- **Location:** same directory as each input file (per [`ARCHITECTURE.md §3.3`](../../ARCHITECTURE.md#33-file-io-conventions)).
- **Naming:** `{input_stem}.{new_ext}`, where `new_ext` matches the target format (`png`, `jpg` for JPEG, `webp`, `bmp`, `tif` for TIFF).
- **Duplicate handling:** `multitool_core::fs::unique_path` resolves collisions to `name (1).ext`, `name (2).ext`, ... Never overwrite silently. This naturally covers the "convert JPEG → JPEG (recompress at lower quality)" case where the output stem matches the input stem.
- **Same-format requests** (e.g., user picks JPEG output for a JPEG input): allowed. Re-encodes with the new quality. Always written through `unique_path` so the original is preserved.

## SVG rasterization

SVG inputs go through `resvg` (pure Rust, no native deps; `usvg` for parsing + `tiny-skia` for rendering — both transitive). The rasterized RGBA buffer then flows through the same encode + alpha-handling pipeline as decoded raster inputs.

- **Size policy** controlled by `svg_raster_size` (see [Options](#options)).
- **`natural`** — uses the SVG's intrinsic `width`/`height`. If both are missing or non-pixel units, fall back to the `viewBox` dimensions at 1:1 (CSS px). If `viewBox` is also missing, the file is `UnsupportedFormat` (no sane default).
- **`longest-edge-px:N`** — scales the intrinsic size so the longest side is exactly N pixels (aspect ratio preserved).
- **Fonts:** load no system fonts (build cost + cross-platform inconsistency not worth it for this tool). SVG text using system fonts renders with `usvg`'s fallback behavior — a warning is emitted on the file (`"SVG references fonts; text may not render"`) if `usvg` reports any text node during parsing.
- **`<image>` href external resources** are not resolved (no I/O during raster). Embedded data URIs are honored. A warning surfaces if external refs are encountered.

## UX flow

`Dashboard → Image Format Converter → (pick files → stage → choose options → convert → result)`

- During conversion: per-file progress (`Converting 3/12: photo.png → photo.jpg`).
- On completion: summary row with `N converted, M skipped`, output directory of the first successful file (with a "reveal" affordance), expandable error list if `M > 0`, and a "Convert more" reset that preserves the staged list (cleared on demand).
- Cancellation: between files. The currently-encoding file finishes or is abandoned at the next checkpoint; partial files are not written (encoding happens in memory, then atomic write).

## Edge cases

- **Animated GIF input** → decode first frame, emit a UI warning in the progress stream. Per-file warning, not a job-level failure. (A dedicated "decompose GIF into frames" tool is a separate follow-up, not part of this scope.)
- **16-bit PNG / TIFF input → 8-bit lossy target** → downconvert with a warning; document in code that this is expected, not a bug.
- **CMYK JPEG input** → convert to RGB before encoding; the `image` crate does this on decode for JPEG.
- **Non-trivial alpha channel + `alpha_handling = preserve` + alpha-less target** → per-file `AppError::ProcessingFailed { detail: "<path>: target format does not support alpha; choose a flatten option" }`. Surfaced in the batch error list; doesn't abort the job.
- **`alpha_handling = flatten-*` + alpha-having target** (e.g., WebP) → ignore, no warning; the option is a no-op.
- **Unreadable bytes / unknown format** → `AppError::UnsupportedFormat` with the file path in `detail`.
- **Empty staging list** → "Convert" button disabled in UI; backend returns `AppError::ProcessingFailed { detail: "no images to convert" }` if called anyway (defense in depth — mirrors `images_to_pdf::convert`).
- **Encrypted / DRM** — not applicable to image formats. No `AppError::Encrypted` branch.

## Acceptance

- [ ] All output formats in the [scope table](#scope-which-formats) round-trip through a unit test (input fixture → output bytes → re-decode → assert dims + format) for at least one input format pairing each.
- [ ] All input formats (including SVG and ICO) have at least one decode test landing in a known output format.
- [ ] SVG raster size: `natural` honors intrinsic dims; `longest-edge-px:N` produces output with longest side == N within ±1 px tolerance.
- [ ] Alpha handling: `preserve` lands a `ProcessingFailed` in the batch error list for `PNG-with-alpha → JPEG`; `flatten-white` produces a white-background JPEG; same for `flatten-black`.
- [ ] EXIF orientation honored on input (test using the existing `rotated.jpg` fixture from Images → PDF).
- [ ] Animated GIF → first frame with warning surfaced through the progress callback.
- [ ] **Skip + continue:** a 3-file batch where file 2 has un-decodable bytes yields 2 successes, 1 entry in the error list, and the job's overall result is `Ok` (not `Err`).
- [ ] Cancellation mid-batch leaves only fully-written files on disk; no half-written outputs.
- [ ] Output-name collision (`photo.jpg` exists) lands on `photo (1).jpg`.
- [ ] Same-format request (`PNG → PNG`) re-encodes through `unique_path` instead of overwriting.
- [ ] `cargo test -p multitool-core` coverage ≥ 80% on `tools/image_format_converter/{convert.rs, job.rs}` (matches the PDF → Images bar).
- [ ] One Playwright happy-path lane covering the dashboard tile → form → success flow.
- [ ] Pre-PR gates green (fmt, clippy `-D warnings`, cargo test, pnpm lint/typecheck/test, `pnpm tauri build --no-bundle`, `pnpm test:e2e`).
- [ ] CI green on Linux, macOS, Windows.
- [ ] [`ASSIGNMENT.md §7`](../../ASSIGNMENT.md#7-nice-to-haves--follow-ups) "Image format converter" line removed; this brief and the plan doc deleted.
