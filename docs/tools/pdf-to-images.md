# Tool: PDF → Images

> Per-tool brief for the first Phase 1 tool. PM-level scope lives in [../../ASSIGNMENT.md §5.1](../../ASSIGNMENT.md). Implementation pattern lives in [../adding-a-tool.md](../adding-a-tool.md). Architecture rules in [../../ARCHITECTURE.md](../../ARCHITECTURE.md).

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
- **Encrypted PDF:** detect, surface as `UnsupportedFormat` (or a more specific variant if added); do not prompt for password in Phase 1
- **Corrupt PDF:** `ProcessingFailed` with the underlying detail
- **Zero-page PDF:** treat as a failure (don't write an empty output folder)
- **Huge page count (1000+):** progress events must not flood IPC — throttle or batch; UI stays responsive
- **Read-only output directory:** `PermissionDenied`; surface as toast including the path
- **Cancellation mid-render:** stop after the current page; leave already-written pages in place (user decides whether to clean up)
- **Disk full mid-job:** propagate the I/O error; same partial-output policy as cancellation

## Implementation pointers
- PDF rendering: `pdfium-render`
- Image encode: `image` crate
- Pure conversion fn shape (to be confirmed during implementation):
  `convert(pdf_bytes, opts, progress, cancel) -> Result<…, AppError>` — pure, no tauri imports
- This tool is the **first concrete user of [../../src-tauri/src/fs/](../../src-tauri/src/fs/)**. Extract `unique_path` and any folder-creation helper here, then update [../adding-a-tool.md](../adding-a-tool.md) → "Shared surfaces" with the real signatures.
- It is also the first user of the IPC layer for a real job (events + cancellation). The wrapper shape that emerges in `src/lib/` becomes the template for future tools — capture it in [../adding-a-tool.md](../adding-a-tool.md) afterwards.

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
