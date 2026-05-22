# Images → PDF — Build Plan

> Ephemeral working doc. Deleted when the tool ships, alongside [images-to-pdf.md](images-to-pdf.md). The brief is the *what*; this is the *how* and the *where we are*. Update inline as commits land — tick boxes, append notes, record blockers. Architectural decisions that emerge mid-build go to [../../DECISIONS.md](../../DECISIONS.md), not this file.

**Status:** 2026-05-22 — Phase A in progress. A1 landed; A2 (runJob extraction) next.

## Conventions for this doc

- One phase = one logical chunk that leaves `master`-shaped tree green (pre-PR checklist passes). Phases run sequentially; commits *within* a phase can be reordered if dependencies permit.
- Tick `[x]` when committed. Strike through `~~stale~~` if a planned commit gets folded or dropped — never silently delete.
- Each commit listed with the conventional-commit subject I'm aiming for. Adjust subjects when reality demands; the listed scope is the contract, not the wording.
- If a commit grows past ~300 lines of meaningful diff (ignoring lockfiles / generated files), split before pushing. Same rule as the playbook.

---

## Phase A — Shared TS surfaces, with PDF → Images migrated in-step

Goal: extract the shared TS scaffolding the second tool wants, and migrate PDF→Images onto it commit-by-commit so each step leaves all existing tests green. Proves the extractions work on the existing tool before images-to-pdf uses them.

- [x] **A1. `refactor(lib): extract AppErrorEnvelope to src/lib/errors.ts`**
  - Move the `AppErrorEnvelope` type out of [src/lib/tools/pdfToImages.ts](../../src/lib/tools/pdfToImages.ts) into a new [src/lib/errors.ts](../../src/lib/errors.ts).
  - `pdfToImages.ts` re-imports; [src/tools/pdf-to-images/types.ts](../../src/tools/pdf-to-images/types.ts) re-exports unchanged.
  - All existing Vitest + Playwright lanes pass with no test edits.

- [ ] **A2. `refactor(lib): extract runJob IPC helper`**
  - New `src/lib/jobRunner.ts`: `runJob<Opts, Progress, Result>(command, opts, hooks)` owns `jobId` generation, `tool:progress` filter, `AbortSignal → cancel_job`, `try/finally unlisten`.
  - [src/lib/tools/pdfToImages.ts](../../src/lib/tools/pdfToImages.ts) shrinks to a thin call site; its test in `pdfToImages.test.ts` keeps asserting the same observable behavior (mocks may need to move to the `runJob` boundary — fine, just keep coverage equivalent).
  - Add a focused `jobRunner.test.ts` covering: jobId filter, abort → cancel_job, listener unsubscribe on both success and error paths, payload shape passthrough.

- [ ] **A3. `feat(ui): extract JobProgress shared component`**
  - New `src/components/JobProgress.tsx`: props `{ current, total, label?, onCancel }`. Internally renders the shadcn `<Progress>` bar + the "N / total" status text + the Cancel button.
  - [src/tools/pdf-to-images/PdfToImages.tsx](../../src/tools/pdf-to-images/PdfToImages.tsx) consumes it; its `PdfToImages.test.tsx` keeps asserting the same labels/aria.
  - Component-level test on `JobProgress` itself: renders label, computes percent, button calls `onCancel`.

**Phase A exit gate:** `pnpm lint && pnpm test && pnpm typecheck && pnpm test:e2e` and `cargo test -p multitool-core --all-targets` all green. No behavior change observable in the running app.

---

## Phase B — System + capability + dep prep

Goal: get the picker, capability grants, and new dep in place before any new tool code references them.

- [ ] **B1. `feat(system): add pickImageFiles wrapper`**
  - Extend [src/lib/system.ts](../../src/lib/system.ts) with `pickImageFiles(): Promise<string[] | null>` using `open({ multiple: true, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }] })`.
  - Returns `null` on cancel, `string[]` on success (never an empty array — Tauri's dialog suppresses that).
  - No test needed (boundary file, mocked by Playwright); document the contract inline.

- [ ] **B2. `chore(capabilities): grant fs:asset scope for thumbnails`**
  - Add the narrowest possible scope to [src-tauri/capabilities/default.json](../../src-tauri/capabilities/default.json) that lets `convertFileSrc()` resolve picked image paths. **Discovery moment:** confirm whether Tauri 2.x supports per-pick dynamic scope grants vs. requiring a static glob. If only static is viable, scope to image extensions only.
  - Write a [DECISIONS.md](../../DECISIONS.md) entry recording the chosen scope shape and why (mirrors the dialog/opener precedent).

- [ ] **B3. `chore(deps): add @dnd-kit/sortable`**
  - `pnpm add @dnd-kit/sortable @dnd-kit/core` (peer required).
  - DECISIONS entry: why @dnd-kit over native HTML5 / react-beautiful-dnd. Cite a11y + maintenance status.

**Phase B exit gate:** lint + typecheck still green; `pnpm tauri build --no-bundle` still compiles (capability changes can wedge the build).

---

## Phase C — Rust pure logic (`multitool-core`)

Goal: pure conversion + orchestrator with full unit-test coverage, no Tauri imports anywhere in this phase.

- [ ] **C1. `feat(images-to-pdf): convert fn in multitool-core`**
  - New `multitool-core/src/tools/images_to_pdf/{mod.rs, convert.rs}`.
  - `convert(image_bytes_iter, opts, on_page, cancel) -> AppResult<JobSummary>`. Input is an iterator of `(path, bytes)` so the orchestrator handles file I/O. Streams progress per image via `on_page` (same shape as PDF→Images for consistency).
  - Honors EXIF orientation via `image` crate's `with_guessed_format` + orientation transform.
  - Page sizing: `auto-fit` (page dims = image dims at 72 DPI), `a4` / `letter` (image scale-to-fit + center).
  - Uses `printpdf` for assembly.
  - Tests cover: each option, EXIF rotation, cancellation between images, unsupported bytes → `UnsupportedFormat`, empty iterator → `ProcessingFailed`. Target ≥80% line cov.

- [ ] **C2. `feat(images-to-pdf): orchestrator job.rs + tests`**
  - New `multitool-core/src/tools/images_to_pdf/job.rs`. Reads each path, threads bytes into `convert`, writes the final PDF via `unique_path({first_image_dir}/{first_stem}.pdf)`.
  - **Partial-cleanup on cancel:** if `convert` returns `Cancelled`, delete the partial PDF if it was created. The PDF→Images orchestrator keeps partial output; we don't, because a half-PDF is useless.
  - Tests: happy path, cancellation deletes partial output, missing input → `FileNotFound`, typed-error propagation, `unique_path` collision.

**Phase C exit gate:** `cargo test -p multitool-core --all-targets` green + `cargo llvm-cov --summary-only -p multitool-core` shows ≥80% on `convert.rs`.

---

## Phase D — Rust shell (Tauri commands)

Goal: thinnest viable shim that calls the orchestrator and emits events. Decide on extracting the shim helper *after* writing the new one.

- [ ] **D1. `feat(images-to-pdf): Tauri command + register`**
  - New `src-tauri/src/tools/images_to_pdf/mod.rs`. Pattern from [src-tauri/src/tools/pdf_to_images/mod.rs](../../src-tauri/src/tools/pdf_to_images/mod.rs): register JobId, `spawn_blocking`, wire `on_page` → `app.emit("tool:progress", ...)`, emit `tool:complete` / `tool:error` after join.
  - Wire into `register_commands` (one-line edit).

- [ ] **D2. `refactor(ipc): extract run_blocking_job shell helper`** *(decision point — fold into D1 or drop entirely)*
  - After writing D1, diff the two shell shims (this one + PDF→Images). If the shape matches closely enough that ~80% of both is identical: extract a `run_blocking_job(app, job_id, work)` helper and migrate both tools. If shapes diverge (e.g. event payload differences worth keeping local): drop this commit and leave the shims inline; revisit on tool #3. Record the call in the commit message either way.

**Phase D exit gate:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` and `pnpm tauri build --no-bundle` green.

---

## Phase E — Frontend

Goal: tool view with staging area, reorder, add-more, remove, and the Create-PDF flow. Component stays presentational; everything OS-touching routes through `src/lib/`.

- [ ] **E1. `feat(images-to-pdf): IPC wrapper imagesToPdf.ts + tests`**
  - New [src/lib/tools/imagesToPdf.ts](../../src/lib/tools/imagesToPdf.ts) built on `runJob` from Phase A. Mirrors PDF→Images' wrapper shape.
  - Vitest covering: invokes the right command, JobId filter on progress, AbortSignal → `cancel_job`, error envelope round-trip.

- [ ] **E2. `feat(images-to-pdf): scaffold tool view, picker → staging state`**
  - New `src/tools/images-to-pdf/{index.ts, ImagesToPdf.tsx, types.ts}`. ViewState: `idle` → `staging`. "Add images" button calls `pickImageFiles`, transitions to `staging`. No reorder / remove yet — just render the picked list of filenames.
  - **Not yet registered in `registry.ts`** (deferred to Phase F so the dashboard test stays green until the tool is feature-complete).

- [ ] **E3. `feat(images-to-pdf): thumbnail grid with remove + drag-reorder`**
  - Replace the plain filename list with a `@dnd-kit/sortable` grid of thumbnail cards. Each card: `<img src={convertFileSrc(path)} />`, per-card remove button.
  - Reorder is mouse + keyboard. Add-more appends to the list.
  - Empty-after-removal returns to `idle`.

- [ ] **E4. `feat(images-to-pdf): page-size option + Create PDF + progress/done/error`**
  - Page-size radio group (`auto-fit` / `a4` / `letter`).
  - "Create PDF" calls the wrapper, transitions through `running` → `done | error`. Reuses `<JobProgress>` from Phase A. Error state preserves the staging list per the brief.
  - "Open output folder" + "Convert another" on `done`.

- [ ] **E5. `feat(images-to-pdf): React component tests`**
  - Vitest + Testing Library: defaults render, picker → staging transition, reorder updates output-name preview, remove control, empty-list → idle, options forwarded, progress text renders, error envelope renders, Cancel aborts the signal. Mock `@/lib/*`.

**Phase E exit gate:** `pnpm lint && pnpm test && pnpm typecheck` green; manual smoke via `pnpm tauri dev` confirms picker → staging → reorder → create → done works end-to-end on Linux.

---

## Phase F — Registry + e2e

- [ ] **F1. `feat(images-to-pdf): register in src/tools/registry.ts + Dashboard test update`**
  - Add import + entry to [src/tools/registry.ts](../../src/tools/registry.ts). Update [src/app/Dashboard.test.tsx](../../src/app/Dashboard.test.tsx) to assert the new tile.
  - This is the only edit to shared frontend files (modulo registry-shaped tests).

- [ ] **F2. `test(e2e): images-to-pdf happy path`**
  - New Playwright spec: dashboard → tile → pick (mocked) → staging → reorder one item → Create PDF → done. Failure paths stay at the unit level.
  - Add a mock under `tests/e2e/mocks/` mirroring `imagesToPdf.ts`; extend the Vite alias map.

**Phase F exit gate:** full pre-PR checklist per [../../CLAUDE.md](../../CLAUDE.md) green locally — fmt → clippy (workspace) → `cargo test -p multitool-core --all-targets` → pnpm lint/typecheck/test → `pnpm tauri build --no-bundle` → `pnpm test:e2e`.

---

## Phase G — Ship

- [ ] **G1. `docs: retire images-to-pdf brief + plan`**
  - Delete this file and [images-to-pdf.md](images-to-pdf.md) once the tool is merged. The brief said "self-describing in code" — at this point the code, tests, and DECISIONS entries should carry the load.
  - Open PR against `master`. CI green across all three OSes before requesting review / self-merge.

---

## Open questions / blockers

*(Append as they surface. Resolve and strike through. If a resolution changes the brief, edit the brief — not just the note here.)*

- **B2 — Tauri 2.x dynamic vs. static `fs:asset` scope.** Need to confirm during implementation. If only static globs are supported, capture in DECISIONS and document the trade-off (slightly broader filesystem visibility from the webview than ideal, but bounded to image extensions).
- **D2 — shim-extraction decision.** Defer until D1 lands; commit message records the call.

## Log

*(One line per noteworthy event: phase boundary, discovery moment, scope shift. Newest first.)*

- 2026-05-22 — A1 landed: `AppErrorEnvelope` moved to [../../src/lib/errors.ts](../../src/lib/errors.ts); `pdfToImages.ts` now re-exports so `types.ts` + e2e mock import paths stay unchanged. All gates green.
- 2026-05-22 — Plan written; branch `feat/images-to-pdf` ready, brief landed. Phase A starts on user go-ahead.
