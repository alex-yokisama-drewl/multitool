# Images → PDF — Build Plan

> Ephemeral working doc. Deleted when the tool ships, alongside [images-to-pdf.md](images-to-pdf.md). The brief is the *what*; this is the *how* and the *where we are*. Update inline as commits land — tick boxes, append notes, record blockers. Architectural decisions that emerge mid-build go to [../../DECISIONS.md](../../DECISIONS.md), not this file.

**Status:** 2026-05-22 — Phase E5 complete (10 component tests, 41/41 vitest). Phase E gate met on automated checks; manual `pnpm tauri dev` smoke not possible from Claude (no interactive UI) — flagging for human smoke before/after Phase F. Phase F (registry + Playwright happy path) next.

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

- [x] **A2. `refactor(lib): extract runJob IPC helper`**
  - New `src/lib/jobRunner.ts`: `runJob<Opts, Progress, Result>(command, opts, hooks)` owns `jobId` generation, `tool:progress` filter, `AbortSignal → cancel_job`, `try/finally unlisten`.
  - [src/lib/tools/pdfToImages.ts](../../src/lib/tools/pdfToImages.ts) shrinks to a thin call site; its test in `pdfToImages.test.ts` keeps asserting the same observable behavior (mocks may need to move to the `runJob` boundary — fine, just keep coverage equivalent).
  - Add a focused `jobRunner.test.ts` covering: jobId filter, abort → cancel_job, listener unsubscribe on both success and error paths, payload shape passthrough.

- [x] **A3. `feat(ui): extract JobProgress shared component`**
  - New `src/components/JobProgress.tsx`: props `{ current, total, label?, onCancel }`. Internally renders the shadcn `<Progress>` bar + the "N / total" status text + the Cancel button.
  - [src/tools/pdf-to-images/PdfToImages.tsx](../../src/tools/pdf-to-images/PdfToImages.tsx) consumes it; its `PdfToImages.test.tsx` keeps asserting the same labels/aria.
  - Component-level test on `JobProgress` itself: renders label, computes percent, button calls `onCancel`.

**Phase A exit gate:** `pnpm lint && pnpm test && pnpm typecheck && pnpm test:e2e` and `cargo test -p multitool-core --all-targets` all green. No behavior change observable in the running app.

---

## Phase B — System + capability + dep prep

Goal: get the picker, capability grants, and new dep in place before any new tool code references them.

- [x] **B1. `feat(system): add pickImageFiles wrapper`**
  - Extend [src/lib/system.ts](../../src/lib/system.ts) with `pickImageFiles(): Promise<string[] | null>` using `open({ multiple: true, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }] })`.
  - Returns `null` on cancel, `string[]` on success (never an empty array — Tauri's dialog suppresses that).
  - No test needed (boundary file, mocked by Playwright); document the contract inline.

- [x] **B2. `feat(asset-scope): dynamic per-pick image-preview grant`** *(scope reshaped during discovery; see Log)*
  - Tauri 2.x asset-protocol scope is `app.security.assetProtocol` in [tauri.conf.json](../../src-tauri/tauri.conf.json), not a capability/permission. The brief's "fs:asset" was a misnomer.
  - Dynamic per-pick chosen: empty static scope + new `allow_image_preview` command in [../../src-tauri/src/asset_scope.rs](../../src-tauri/src/asset_scope.rs) that re-validates extensions and calls `allow_file` per path. Registered in [tools/mod.rs](../../src-tauri/src/tools/mod.rs); `tauri` crate gained the `protocol-asset` feature (required by build).
  - DECISIONS entry written: [../../DECISIONS.md](../../DECISIONS.md) → "Asset protocol scope: dynamic per-pick".
  - Frontend wrapper deferred to E2 (the call site that needs it).

- [x] **B3. `chore(deps): add @dnd-kit/sortable`**
  - `pnpm add @dnd-kit/sortable @dnd-kit/core` (peer required).
  - DECISIONS entry: why @dnd-kit over native HTML5 / react-beautiful-dnd. Cite a11y + maintenance status.

**Phase B exit gate:** lint + typecheck still green; `pnpm tauri build --no-bundle` still compiles (capability changes can wedge the build).

---

## Phase C — Rust pure logic (`multitool-core`)

Goal: pure conversion + orchestrator with full unit-test coverage, no Tauri imports anywhere in this phase.

- [x] **C1. `feat(images-to-pdf): convert fn in multitool-core`**
  - New `multitool-core/src/tools/images_to_pdf/{mod.rs, convert.rs}`.
  - `convert(&[(PathBuf, Vec<u8>)], opts, on_page, cancel) -> Result<(Vec<u8>, JobSummary), AppError>` — bytes returned to the caller (orchestrator) so it owns the disk write; means C2's partial-cleanup-on-cancel is trivially satisfied (no file ever created on the error path).
  - Honors EXIF orientation via `ImageReader::with_guessed_format → into_decoder → orientation() → DynamicImage::from_decoder → apply_orientation`. Missing tag → `NoTransforms` (best-effort).
  - Page sizing: `AutoFit` (page = image dims at 72 DPI), `A4` / `Letter` (image scale-to-fit + centered, aspect preserved).
  - Uses `printpdf 0.7` with `default-features = false` (avoid the HTML/azul pipeline). `ImageXObject` built directly from `image::DynamicImage::into_rgb8` raw bytes, so the printpdf-internal `image 0.24` vs our `image 0.25` doesn't matter.
  - 12 unit tests cover: each `PageSize`, EXIF rotation (orientation 6 swaps wide→tall), cancellation between + before, `on_page` halts, `UnsupportedFormat` on garbage bytes, `ProcessingFailed` on empty slice, webp input round-trips. **92.26% line cov on `convert.rs`** (gate ≥80%).

- [x] **C2. `feat(images-to-pdf): orchestrator job.rs + tests`**
  - New `multitool-core/src/tools/images_to_pdf/job.rs`. Reads each input path into memory, hands `&[(PathBuf, Vec<u8>)]` to `convert`, then writes the returned PDF bytes to `unique_path({first_image_parent}/{first_image_stem}.pdf)`. Pre-read cancel-check + the cancel-check inside `convert` between images give two opportunities to bail.
  - **Partial-cleanup on cancel:** satisfied by construction. Because the orchestrator only writes after `convert` returns the full PDF bytes, no output file is ever created on the `Cancelled` (or any other error) path — there is no partial to delete. The plan's original "delete the partial PDF if it was created" reads as a fallback for a writer-arg design; with bytes-back-to-caller it's redundant.
  - `Progress { image: u32, total: u32 }` (1-based, matches "image N / total" UX copy). `JobResult { output_path, page_count, duration_ms }`.
  - 10 tests: happy path (2 images, on-disk file, progress order), first-image-dir wins with mixed-folder inputs, missing input → `FileNotFound`, pre-cancel → no output file, mid-convert cancel → no output file, `unique_path` collision leaves existing untouched, on_progress halts + no output, `UnsupportedFormat` propagation from convert + no output, empty slice → `ProcessingFailed`, `derive_output_path` path-math cases.

**Phase C exit gate:** `cargo test -p multitool-core --all-targets` (54 tests) and `cargo llvm-cov --summary-only -p multitool-core` (convert.rs 92.26%, job.rs 89.95%) both green.

---

## Phase D — Rust shell (Tauri commands)

Goal: thinnest viable shim that calls the orchestrator and emits events. Decide on extracting the shim helper *after* writing the new one.

- [x] **D1. `feat(images-to-pdf): Tauri command + register`**
  - New `src-tauri/src/tools/images_to_pdf/mod.rs`. Mirrors `src-tauri/src/tools/pdf_to_images/mod.rs` deliberately so the D2 decision diff is clean: registers JobId, `spawn_blocking`, wires `on_progress` → `app.emit("tool:progress", ...)`, emits `tool:complete` / `tool:error` after join.
  - Wired into `register_commands` with a single import + handler entry.

- [x] **D2. `refactor(ipc): extract run_blocking_job shell helper`** — **dropped.**
  - After writing D1, diffed the two shims: shapes match closely (~60 of ~105 lines identical: ProgressEvent/CompleteEvent/ErrorEvent structs, registry register/unregister, spawn_blocking + await + join-error map, complete/error emit). Per-tool differences are the function name, the input-arg shape (`PathBuf` vs `Vec<PathBuf>`), and the closure body that calls multitool-core's `run_job`.
  - Decision: leave the shims inline. CLAUDE.md is explicit ("Three similar lines is better than a premature abstraction") and even 60 lines × 2 sites is only the second occurrence. A helper would need non-trivial trait bounds (closure `Send + 'static`, generic Progress/Result with `Serialize` bounds, emit calls reaching app+job_id from inside the closure) — abstraction tax that two linearly-readable shims don't pay. The plan offered "revisit on tool #3" as the explicit escape; that's where the rule-of-three evidence will be unambiguous.

**Phase D exit gate:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` and `pnpm tauri build --no-bundle` green.

---

## Phase E — Frontend

Goal: tool view with staging area, reorder, add-more, remove, and the Create-PDF flow. Component stays presentational; everything OS-touching routes through `src/lib/`.

- [x] **E1. `feat(images-to-pdf): IPC wrapper imagesToPdf.ts + tests`**
  - New [src/lib/tools/imagesToPdf.ts](../../src/lib/tools/imagesToPdf.ts) built on `runJob` from Phase A. Mirrors PDF→Images' wrapper shape: `PageSize` (kebab-case mirror of the Rust enum), `Opts { page_size }`, `JobResult { output_path, page_count, duration_ms }`, `Progress { image, total }`, `AppErrorEnvelope` re-export.
  - 7 Vitest cases mirror `pdfToImages.test.ts`: invokes right command + args; forwards progress filtered by JobId; unsubscribes on success + on error; AbortSignal → cancel_job invoke; typed-error envelope round-trip; pre-aborted signal throws without invoke / listen.

- [x] **E2. `feat(images-to-pdf): scaffold tool view, picker → staging state`**
  - New `src/tools/images-to-pdf/{index.ts, ImagesToPdf.tsx, types.ts}`. ViewState: `idle` | `staging` (E3/E4 will widen this). "Add images" triggers picker → `allowImagePreview` grant → sort-by-filename ascending → staging list of filenames. Add-more appends + re-sorts. Esc returns to dashboard, matching PdfToImages.
  - B2 frontend wrapper landed alongside: `allowImagePreview(paths)` added to [src/lib/system.ts](../../src/lib/system.ts). Granting at pick-time so E3 can render `<img src={convertFileSrc(path)}>` without further IPC ceremony.
  - **Not yet registered in `registry.ts`** — deferred to Phase F so the dashboard test stays green until the tool is feature-complete.

- [x] **E3. `feat(images-to-pdf): thumbnail grid with remove + drag-reorder`**
  - Filename list replaced with a responsive grid (2/3/4 cols) of thumbnail cards rendering `<img src={convertFileSrc(path)} />`. The card body is a `<button>` wired as the dnd-kit drag handle (so keyboard focus + Space activates reorder); a separate × button (with `stopPropagation` so clicks never start a drag) removes the item.
  - `DndContext` with `PointerSensor` + `KeyboardSensor` (using `sortableKeyboardCoordinates`); `SortableContext` with `rectSortingStrategy`; `arrayMove` on `onDragEnd`.
  - Items wrapped in `{ id, path }` shape (stable UUID id) so duplicate paths — which the brief explicitly allows — don't collide as dnd-kit keys. `id` never reaches IPC; only `path` does.
  - Add-more appends new items + re-sorts; remove dropping last item → `idle`.
  - CSS transform serialized inline (`translate3d(${x}px, ${y}px, 0)`) — avoids adding `@dnd-kit/utilities` as a third dnd-kit dep beyond the two B3 planned for.

- [x] **E4. `feat(images-to-pdf): page-size option + Create PDF + progress/done/error`**
  - Page-size radio group below the grid (`Auto-fit (per image)` / `A4` / `Letter`); `pageSize` lives in its own `useState` so the selection survives error → staging round-trips.
  - "Create PDF" calls `convertImagesToPdf` with `items.map(path)` + `{ page_size }`. State machine adds `running` (reuses `<JobProgress label="image" />`) → `done` (path display + "Open output folder" via `revealInFolder` + "Convert another" via reset).
  - Error path folded into `staging` (added optional `error: AppErrorEnvelope` field) so the items list is preserved and the user can retry without re-picking — the brief's "kept in staging state" rule. Cancellation surfaces as a Cancelled envelope through the same path; a follow-up polish could suppress that specific kind from the alert.
  - `Create PDF` disabled when items is empty (defensive; staging is unreachable with zero items, but keeps the button's disabled-state contract obvious).

- [x] **E5. `feat(images-to-pdf): React component tests`**
  - 10 Vitest + Testing Library cases mock `@/lib/system`, `@/lib/tools/imagesToPdf`, and `@tauri-apps/api/core` (the convertFileSrc seam): idle defaults render; picker → staging with allowImagePreview called; cancel-picker stays in idle; filename-ascending sort drives the output-name preview; × removes + empty-list → idle; page-size defaults to auto-fit and a different choice forwards through; streaming progress renders "image N / total"; error envelope folds back into staging with items preserved; Cancel button aborts the captured AbortSignal; revealInFolder receives the output_path on done.
  - Added a small "Output: {first_stem}.pdf" preview (with `data-testid="output-preview"`) so the sort-driven first-item behaviour has a single UI surface to assert against. Mouse + keyboard drag-reorder of the dnd-kit grid itself is deferred to Playwright (E2E layer) — jsdom doesn't reliably simulate pointer-rect collision math.

**Phase E exit gate:** automated checks (`pnpm lint && pnpm test && pnpm typecheck` + `pnpm exec prettier --check`) all green: 41/41 vitest, no type errors, no new lint errors. Interactive `pnpm tauri dev` smoke (picker → staging → reorder → create → done) is left for human review — Claude can't drive the GUI.

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

- ~~**B2 — Tauri 2.x dynamic vs. static `fs:asset` scope.**~~ Resolved 2026-05-22: dynamic is viable via `Manager::asset_protocol_scope().allow_file(...)`. Picked dynamic; the brief's "fs:asset" was a misnomer (asset protocol is core, not an `fs:` permission). See DECISIONS.
- ~~**D2 — shim-extraction decision.**~~ Resolved 2026-05-22: dropped. Shims kept inline; revisit when tool #3 lands. See D2 entry above.

## Log

*(One line per noteworthy event: phase boundary, discovery moment, scope shift. Newest first.)*

- 2026-05-22 — E5 landed: 10 Vitest cases on `ImagesToPdf.test.tsx` covering idle defaults, picker → staging, picker-cancel, filename-ascending sort + output preview, ×-remove + empty → idle, page-size default + forward, progress text, error envelope folded into staging, Cancel aborts the signal, revealInFolder on done. Small "Output: {first_stem}.pdf" preview added to staging (sort + reorder coverage hook). 41/41 vitest, typecheck + lint + format clean. Manual `pnpm tauri dev` smoke deferred to human review — Claude can't drive the GUI.
- 2026-05-22 — E4 landed: page-size radio (Auto-fit / A4 / Letter) + Create PDF wiring through running → done with `<JobProgress label="image" />`. Error folded into `staging` with optional `error` field so the items list is preserved per the brief. "Open output folder" calls `revealInFolder(output_path)`; "Convert another" resets to idle + restores default page size. AbortController pattern matches PdfToImages. 31/31 vitest, typecheck + lint + format clean. (E5 component tests next, then manual smoke at the phase exit gate.)
- 2026-05-22 — E3 landed: thumbnail grid via @dnd-kit/sortable, mouse + keyboard reorder, per-card × remove (with stopPropagation), add-more appends + re-sorts, empty list → idle. Items wrapped as `{ id: uuid, path }` so duplicate paths work (brief explicitly allows). Inline transform string instead of pulling in @dnd-kit/utilities — keeps the dnd-kit dep count at the two B3 planned for. 31/31 vitest still green, typecheck + lint clean.
- 2026-05-22 — E2 landed: `src/tools/images-to-pdf/{index.ts, types.ts, ImagesToPdf.tsx}` (~85 lines total). Idle → staging via picker, add-more, filename-ascending sort on every pick. `allowImagePreview()` wrapper added to `src/lib/system.ts` and called at pick-time (B2 wrapper resolved). Tool deliberately NOT in `registry.ts` yet — that's F1's job. 31/31 vitest still green, typecheck + lint clean.
- 2026-05-22 — E1 landed: `src/lib/tools/imagesToPdf.ts` (~50 lines) + 7 Vitest cases mirroring the pdfToImages wrapper. PageSize is kebab-case ("auto-fit" / "a4" / "letter") to match the Rust enum's `#[serde(rename_all = "kebab-case")]`; field names verbatim (`page_size`, `output_path`). 31/31 vitest green workspace-wide, typecheck + lint clean.
- 2026-05-22 — D1 landed: `src-tauri/src/tools/images_to_pdf/mod.rs` (~106 lines) + registry wire-up; `pnpm tauri build --no-bundle` builds in release. D2 dropped — shims kept inline per CLAUDE.md's "rule of three" guidance, plan's "revisit on tool #3" escape hatch, and the abstraction-tax cost of a generic helper (closure Send + 'static + Serialize bounds reaching app+job_id from inside the closure). Phase D exit gate met: fmt + clippy workspace + no-bundle build all green.
- 2026-05-22 — C2 landed: `multitool-core/src/tools/images_to_pdf/job.rs` with `run_job()`, `Progress { image, total }`, `JobResult`. 10 unit tests; job.rs at 89.95% line cov. Phase C exit gate met (54 tests green across multitool-core; convert.rs 92.26%, job.rs 89.95%). **Design note:** the plan's "delete the partial PDF on cancel" simplified to "never create one" — because `convert` returns bytes to the caller and the orchestrator writes only after success, no partial PDF ever exists. Recorded in the C2 checkbox text so future readers don't go looking for the delete logic.
- 2026-05-22 — C1 landed: `multitool-core/src/tools/images_to_pdf/{mod.rs, convert.rs}` with `convert()` returning `Result<(Vec<u8>, JobSummary), AppError>` (bytes back to caller — keeps disk I/O in C2's orchestrator and means cancel never leaves a partial PDF). printpdf 0.7 with `default-features = false`, image gains `webp` feature. EXIF orientation honored via decoder.orientation() + apply_orientation; rotated.jpg fixture (orientation 6) verifies the wide→tall swap. 12 unit tests, 92.26% line cov on convert.rs (gate ≥80%). Phase C exit gate not yet met — C2 still pending.
- 2026-05-22 — B3 landed: `@dnd-kit/sortable` + `@dnd-kit/core` added. DECISIONS entry "Staging-area reorder: @dnd-kit/sortable over native HTML5 / react-beautiful-dnd" recorded (a11y mandate + react-beautiful-dnd archived + dep-discipline trade-off acknowledged). Phase B exit gate green (`pnpm tauri build --no-bundle` confirms no wedge from the new deps).
- 2026-05-22 — **Discovery + B2 landed.** Brief's "fs:asset" was a misnomer (no such Tauri 2.x permission); real grant is `app.security.assetProtocol` in `tauri.conf.json`. Dynamic per-pick IS supported via `Manager::asset_protocol_scope().allow_file(...)`. Picked dynamic. New `allow_image_preview` command validates extensions server-side and calls `allow_file` per picked path. Required adding `protocol-asset` to the `tauri` crate features. DECISIONS entry recorded. Frontend wrapper deferred to E2. Exit gate green (rust fmt/clippy/test + `pnpm tauri build --no-bundle` + frontend lint/typecheck).
- 2026-05-22 — B1 landed: `pickImageFiles()` added to [../../src/lib/system.ts](../../src/lib/system.ts). Multi-select, `.png/.jpg/.jpeg/.webp` filter, returns `null` on cancel / `string[]` on confirm. No unit test per plan (boundary file, mocked by Playwright).
- 2026-05-22 — A3 landed: `JobProgress` extracted to [../../src/components/JobProgress.tsx](../../src/components/JobProgress.tsx) (props `{ current, total, label?, onCancel }`); handles the pre-first-event "starting…" case internally so Cancel stays available throughout `running`. PdfToImages threads through with `label="page"`. 5 new component tests; existing PdfToImages tests pass untouched. Phase A gate (lint/test/typecheck/e2e) green.
- 2026-05-22 — A2 landed: `runJob<Args, Progress, Result>` extracted to [../../src/lib/jobRunner.ts](../../src/lib/jobRunner.ts); `pdfToImages.ts` is now a one-call wrapper. New `jobRunner.test.ts` covers jobId filter / abort / unlisten / payload passthrough; existing `pdfToImages.test.ts` kept passing untouched (mocks still at `@tauri-apps/api/*`, which `runJob` calls internally). 19/19 vitest green.
- 2026-05-22 — A1 landed: `AppErrorEnvelope` moved to [../../src/lib/errors.ts](../../src/lib/errors.ts); `pdfToImages.ts` now re-exports so `types.ts` + e2e mock import paths stay unchanged. All gates green.
- 2026-05-22 — Plan written; branch `feat/images-to-pdf` ready, brief landed. Phase A starts on user go-ahead.
