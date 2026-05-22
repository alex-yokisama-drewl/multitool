# Tool: Images → PDF

> Ephemeral. Delete when the tool ships. PM-level scope: [../../ASSIGNMENT.md §5.2](../../ASSIGNMENT.md#52-tool-images--pdf). Playbook: [../adding-a-tool.md](../adding-a-tool.md).

## Summary

Assemble an ordered list of image files into a single PDF, one image per page, with a reorderable staging area before generation.

## Inputs

- One or more image files (`.png`, `.jpg`, `.jpeg`, `.webp`), selected via OS picker with multi-select enabled.
- Add-more picker appends to the existing list (does not clear). Images may come from different folders in a single PDF.
- Initial order on each pick batch: filename ascending.
- No drag-and-drop input from the OS in 0.1.0 — that's the [ASSIGNMENT §5](../../ASSIGNMENT.md#5-phase-1-scope) stretch goal slipping to 0.2.0.

## Options

| Option    | Type                          | Default                   | Notes                                                                                  |
| --------- | ----------------------------- | ------------------------- | -------------------------------------------------------------------------------------- |
| Page size | `auto-fit` \| `a4` \| `letter` | `auto-fit`               | `auto-fit`: each page sized to its image. `a4`/`letter`: image scaled-to-fit + centered on the standard page, aspect ratio preserved, no padding option exposed. |

No padding control. No quality / DPI option in 0.1.0 — images are embedded as-decoded; revisit if file-size complaints come up.

## Output

- **Location:** directory of the first image in the list at the moment "Create PDF" is clicked (post-reorder).
- **Naming:** `{first_image_stem}.pdf`. The "first image" is whichever image sits at index 0 in the reordered list — both the directory and the name follow it.
- **Duplicate handling:** per [../../ARCHITECTURE.md §3.3](../../ARCHITECTURE.md#33-file-io-conventions) — `{stem}.pdf` → `{stem} (1).pdf` on collision via `unique_path`.
- Nothing is written to disk until the user clicks "Create PDF".

## UX flow

Dashboard → Images → PDF → Result. The tool view is a small state machine:

```
idle → picking → staging ⇄ (add-more | remove | reorder) → running → done | error
```

- **`idle`:** "Add images" button.
- **`staging`:** thumbnail grid (rendered via Tauri's `convertFileSrc()` asset protocol, scoped capability), each card has a remove (×) control; the grid is a `@dnd-kit/sortable` list (mouse + keyboard reorder). Page-size selector + "Create PDF" + "Add more images" are persistent here.
- **`running`:** per-image progress (image N / total), Cancel button. Cancellation deletes any partially-written PDF so we never leave a half-file on disk.
- **`done`:** "Wrote PDF to {path}" + "Open output folder" + "Convert another".
- **`error`:** kept in staging state with the existing list preserved + the error envelope rendered above the grid, so the user can retry without re-picking.
- **Esc** returns to dashboard at any stage; **Ctrl/Cmd+O** opens the add-more picker.

Empty list after all-removed returns the UI to `idle` (not back to the dashboard).

## Edge cases

- **EXIF orientation honored** — phone JPEGs commonly carry orientation metadata; without honoring it pages come out sideways.
- **Duplicate images allowed** — user may intentionally repeat a page; don't dedupe.
- **File missing at Create PDF time** (picked then deleted/moved): `AppError::FileNotFound`, error state preserves the rest of the list so the user can remove the bad entry and retry.
- **Unsupported image bytes** (picker filter bypassed, e.g. wrong-extension file): `AppError::UnsupportedFormat`, same retry behavior.
- **Very large images** (e.g. 30000×30000 px): `image` crate may OOM or take seconds; not optimized for 0.1.0. Document, don't gate.
- **Mixed-folder input:** output goes to the *first image's* folder, not a common ancestor — keeps the rule simple and matches ASSIGNMENT.
- **Zero images:** "Create PDF" button disabled when list is empty.

## Shared extractions landing in this PR

Second-tool moment per [../adding-a-tool.md → Shared surfaces](../adding-a-tool.md#shared-surfaces). Extract what's clearly duplicated, leave the borderline cases inline until a third tool shows up.

- `src/lib/errors.ts` — `AppErrorEnvelope` moves here, both tools import from one place.
- `src/lib/jobRunner.ts` — generic `runJob<Opts, Progress, Result>(command, opts, hooks)` that owns the `jobId` generation, `tool:progress` filter, `AbortSignal → cancel_job`, and `try/finally unlisten`. Per-tool wrappers (`pdfToImages.ts`, `imagesToPdf.ts`) become thin shape-typed call sites.
- `src/lib/system.ts` — add `pickImageFiles(): Promise<string[] | null>`.
- `src/components/JobProgress.tsx` — progress bar + "N / total" status + Cancel button.
- PDF → Images is migrated onto the new shared surfaces in the same PR (one bundled PR per the decision recorded above).
- **Deferred** (still rule-of-three): `useJobState` hook, `ResultCard`, `ErrorAlert`. Small enough to inline; revisit on tool #3.
- **Rust:** decide after writing the shell command shim — if the `spawn_blocking + emit progress/complete/error` shape matches PDF→Images closely, extract a `run_blocking_job` helper in `src-tauri/src/ipc/` (or similar). Otherwise leave the shim inline and revisit on tool #3.

## New dependencies

- **`@dnd-kit/sortable`** (+ `@dnd-kit/core` peer) for the staging-area reorder. Needs a [../../DECISIONS.md](../../DECISIONS.md) entry on why this over alternatives.
- **`printpdf`** Rust crate for PDF assembly — already in [../../ARCHITECTURE.md](../../ARCHITECTURE.md#1-tech-stack) tech-stack table; first actual use.
- **Tauri `fs:asset` capability** (or equivalent) scoped to the picked image paths so `convertFileSrc()` can resolve them. Narrowest grant; DECISIONS entry required per the dialog/opener precedent.

## Acceptance

- [ ] Multi-select picker accepts `.png` / `.jpg` / `.jpeg` / `.webp`.
- [ ] Add-more appends to the existing list; supports images from different folders in a single PDF.
- [ ] Drag-and-drop reorder within the staging grid; keyboard reorder works.
- [ ] Per-thumbnail remove control; empty list returns to `idle`.
- [ ] Page-size option (`auto-fit` / `a4` / `letter`); A4 / Letter scale-to-fit + center, aspect ratio preserved.
- [ ] EXIF orientation honored on input images.
- [ ] Per-image progress events emit to the UI as `tool:progress`.
- [ ] Cancel mid-job deletes the partial PDF (no half-files on disk).
- [ ] Output named `{first_image_stem}.pdf` in the first image's directory; collisions go through `unique_path`.
- [ ] Test layers per [../adding-a-tool.md §7](../adding-a-tool.md#7-add-tests): Rust pure + orchestrator unit tests (≥80% line cov on `convert.rs`), TS wrapper Vitest, React component Vitest, Playwright happy-path e2e.
- [ ] PDF → Images migrated to the extracted shared surfaces with no behavior change (existing tests still pass).
- [ ] DECISIONS entries written for `@dnd-kit/sortable` and the `fs:asset` capability grant.
- [ ] Pre-PR checklist (per [../../CLAUDE.md](../../CLAUDE.md)) green locally before opening.
