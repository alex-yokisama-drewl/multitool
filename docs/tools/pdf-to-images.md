# Tool: PDF → Images

> Per-tool brief for the first Phase 1 tool. PM-level scope lives in [../../ASSIGNMENT.md §5.1](../../ASSIGNMENT.md). Implementation pattern lives in [../adding-a-tool.md](../adding-a-tool.md). Architecture rules in [../../ARCHITECTURE.md](../../ARCHITECTURE.md). **Build progress is tracked in [pdf-to-images-plan.md](pdf-to-images-plan.md) (ephemeral working doc — deleted when the tool ships).**

## Summary

Render every page of a PDF as an image file, one image per page, into a sibling subfolder of the input.

## Inputs
- Exactly one PDF file
- Selected via file picker (or `Ctrl/Cmd+O`). Drag-and-drop is a 0.2.0 feature — not required here.

## Options
| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| Format | `png` \| `jpeg` | `png` | JPEG is lossy; useful for very large PDFs |
| DPI | integer | `150` | UI should clamp to a sensible range (e.g. 72–600) |

## Output
- **Location:** same directory as the input PDF
- **Folder name:** `{pdf_stem}_pages/` (e.g. `report.pdf` → `report_pages/`)
- **File name:** `page_{NNN}.{ext}` — zero-padded to at least 3 digits; widen padding if page count exceeds 999
- **Duplicate handling:** if `{pdf_stem}_pages/` already exists, append ` (1)`, ` (2)`, … per [../../ARCHITECTURE.md §3.3](../../ARCHITECTURE.md#33-file-io-conventions). Never merge into an existing folder; never overwrite.

## UX flow
1. Dashboard → "PDF → Images"
2. File picker (or `Ctrl/Cmd+O`); single PDF
3. Tool view: file name, page count (once known), format + DPI controls, "Convert" button
4. Convert → progress bar + per-page counter (`page 12 / 87`); Cancel button visible during the job
5. On complete: result view with "Open output folder" and "Convert another"
6. `Esc` returns to Dashboard from any state. Cancellation mid-job is required (per [../../ARCHITECTURE.md §3.2](../../ARCHITECTURE.md#32-process-model)).

## Edge cases
- **Encrypted PDF:** detect, surface as `AppError::Encrypted` (see [../../DECISIONS.md](../../DECISIONS.md) → "AppError: add `Encrypted` variant"); do not prompt for password in Phase 1
- **Corrupt PDF:** `ProcessingFailed { details }` with the underlying reason
- **Zero-page PDF:** `ProcessingFailed { details: "empty document" }` — don't write an empty output folder
- **Huge page count (1000+):** progress events must not flood IPC — throttle or batch; UI stays responsive
- **Read-only output directory:** `PermissionDenied`; surface as toast including the path
- **Cancellation mid-render:** stop after the current page; leave already-written pages in place (user decides whether to clean up)
- **Disk full mid-job:** propagate the I/O error; same partial-output policy as cancellation

## Implementation pointers
- PDF rendering: `pdfium-render`. **Binary-distribution strategy is the one open decision** — resolved during build-plan step C1 and recorded in [../../DECISIONS.md](../../DECISIONS.md).
- Image encode: `image` crate
- All pure logic (conversion fn + `unique_path` + output writer) lives in `multitool-core`. See [../../DECISIONS.md](../../DECISIONS.md) → "Heavy deps allowed in `multitool-core`".
- Pure conversion fn signature:
  ```rust
  pub fn convert(
      pdf_bytes: &[u8],
      opts: &Opts,
      on_page: impl FnMut(PageOutput) -> Result<(), AppError>,
      cancel: &CancellationToken,
  ) -> Result<JobSummary, AppError>;
  ```
  Streaming via `on_page` callback (not `Vec<PageOutput>`) — see [../../DECISIONS.md](../../DECISIONS.md) → "Streaming `on_page` callback".
- This tool is the **first concrete user of `multitool-core/src/fs.rs`** (pure path helpers like `unique_path`). [../../src-tauri/src/fs/](../../src-tauri/src/fs/) stays reserved for path resolution that genuinely needs Tauri APIs.
- It is also the first user of the IPC layer for a real job (events + cancellation). The wrapper shape that emerges in [../../src/lib/](../../src/lib/) becomes the template for future tools — capture it in [../adding-a-tool.md](../adding-a-tool.md) afterwards (build-plan step C10).

## Acceptance criteria
- [ ] Single PDF → folder of PNG/JPEG pages, named per the rules above
- [ ] DPI and format options honored end-to-end
- [ ] Progress events stream per page; UI shows current page / total
- [ ] Cancel button stops the job; partial output left in place
- [ ] Duplicate output folder gets ` (1)` suffix; no overwrites of any file or folder
- [ ] Encrypted / corrupt / zero-page PDFs surface the correct `AppError` kind
- [ ] Read-only destination surfaces `PermissionDenied` cleanly
- [ ] Tests: Rust unit on `convert.rs` (≥80% line cov), Vitest component smoke, Playwright happy-path
- [ ] Pre-PR checklist from [../../CLAUDE.md](../../CLAUDE.md) green on all three OSes
