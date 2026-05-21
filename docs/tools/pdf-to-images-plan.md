# PDF → Images — Build Plan

> **Working doc — ephemeral.** Tracks the build sequence for the PDF→Images tool. The permanent brief is [pdf-to-images.md](pdf-to-images.md); architectural decisions live in [../../DECISIONS.md](../../DECISIONS.md). When all commits ship, follow the "When this is done" checklist at the bottom and **delete this file**.

## Working agreement (for the agent executing this plan)

- **Mark commits `[x]` as soon as they merge.** Don't batch.
- **Edit this file when scope shifts.** If a commit splits, merges, gets reordered, or is dropped — update the list. This doc should always reflect the actual path forward, not the original plan. Stale steps are worse than no steps.
- **Architectural choices go to [../../DECISIONS.md](../../DECISIONS.md), not here.** This file is the checklist; the why-history lives in DECISIONS.
- **One open question at a time.** If you hit a fork that isn't already resolved in DECISIONS, surface it in chat with the user before committing — same protocol as the original planning session.
- **Per-PR checklist still applies.** Every commit gates on fmt, clippy, pnpm lint/typecheck/test, and `pnpm tauri build --no-bundle`. See [../../CLAUDE.md → Per-PR checklist](../../CLAUDE.md). Lefthook enforces a subset on commit; the rest is your responsibility before pushing.

## Resolved up front

Settled in the 2026-05-21 DECISIONS entries — read those before starting:
- AppError gets a new `Encrypted` variant; corrupt + zero-page reuse `ProcessingFailed { details }`
- Pure conversion fns live in `multitool-core` even with heavy deps (pdfium)
- Conversion fn API: streaming `on_page` callback, not `Vec<PageOutput>`
- Test fixtures are small real PDFs checked into `multitool-core/tests/fixtures/`

The one decision still TBD: **pdfium binary distribution strategy** — resolved during C1 and recorded in DECISIONS at that point.

## Commits

### [x] C1 — chore(deps): add pdfium-render + image, pdf-open smoke test
**Spike — retired the pdfium binary risk.**
- `pdfium-render = "0.9"` + `image = "0.25"` added to `multitool-core/Cargo.toml`
- Strategy: dynamic-load via `build.rs` download from bblanchon/pdfium-binaries at the pinned `chromium/7763` tag; library path baked into the lib via `PDFIUM_LIB_PATH` env var. See [../../DECISIONS.md](../../DECISIONS.md) → "pdfium binary: dynamic-load via `build.rs` download".
- Fixture `multitool-core/tests/fixtures/three-page.pdf` (580 B) committed alongside the generator at `scripts/gen_pdf_fixture.py`.
- Smoke test at `multitool-core/tests/pdfium_smoke.rs` opens the fixture and asserts `page_count == 3` — passes locally; CI verifies on linux/macos/windows.
- **C6 carryover:** runtime path resolution for bundled releases is unsolved — captured in the DECISIONS entry's "Phase-1 gap" paragraph.

### [ ] C2 — feat(core): AppError::Encrypted variant
- Add `Encrypted` variant to `multitool_core::error::AppError` (no payload)
- Update serialization mapping + frontend type mirror if applicable
- **Tests:** serialization round-trip for the new variant; mirrors existing AppError tests.

### [ ] C3 — feat(core): pdf-to-images pure conversion
- Module: `multitool-core/src/tools/pdf_to_images/{mod.rs, convert.rs}`
- Public API (final):
  ```rust
  pub struct Opts { pub format: Format, pub dpi: u32 }
  pub enum Format { Png, Jpeg }
  pub struct PageOutput { pub index: u32, pub encoded: Vec<u8> }
  pub struct JobSummary { pub page_count: u32, pub duration: Duration }

  pub fn convert(
      pdf_bytes: &[u8],
      opts: &Opts,
      on_page: impl FnMut(PageOutput) -> Result<(), AppError>,
      cancel: &CancellationToken,
  ) -> Result<JobSummary, AppError>;
  ```
- Check in remaining fixtures: `encrypted.pdf`, `corrupt.pdf`, `single-page.pdf` (≤ 20 KB each)
- **Tests (target ≥80% line cov on `convert.rs`):**
  - PNG output: callback fires N times; bytes start with PNG magic
  - JPEG output: same, JPEG magic
  - DPI 72 vs 300: callback receives different `encoded.len()` / decoded dimensions
  - Encrypted PDF → `Err(AppError::Encrypted)`
  - Corrupt PDF → `Err(AppError::ProcessingFailed { .. })`
  - Zero-page PDF → `Err(AppError::ProcessingFailed { .. })`
  - Cancellation after page 1 → callback fires once, then `Err(AppError::Cancelled)`
  - `on_page` returning `Err` → conversion halts and propagates the error
  - DPI clamp (or rejection) — confirm the API contract during impl

### [ ] C4 — feat(core): unique_path helper for duplicate-name policy
- Module: `multitool-core/src/fs.rs`
- API: `pub fn unique_path(target: &Path) -> std::io::Result<PathBuf>` — returns the input as-is if free; otherwise appends ` (1)`, ` (2)`, … to the stem (files) or full name (dirs). Implements the policy from [../../ARCHITECTURE.md §3.3](../../ARCHITECTURE.md#33-file-io-conventions).
- Single-threaded use only — document that fact; not race-safe by design.
- **Tests (tempdir):**
  - Target doesn't exist → returns unchanged
  - File exists → returns `name (1).ext`
  - File + `name (1).ext` both exist → returns `name (2).ext`
  - Directory exists → returns `name (1)/`
  - Stem with multiple dots (`foo.tar.gz`) → suffix lands before `.gz`? Decide + test.
  - No extension (`Makefile`) → suffix appended to full name

### [ ] C5 — feat(core): pdf-pages output writer
- Module: `multitool-core/src/tools/pdf_to_images/writer.rs`
- API consumes a `PageOutput` stream and a target dir → writes `page_NNN.{ext}` files. Resolves the target dir through `unique_path`. Zero-padding widens past 999 pages based on declared total (or post-hoc rename if total unknown — decide).
- **Tests (tempdir):**
  - 3-page job → 3 files named `page_001..page_003.png`
  - Padding widens for ≥ 1000 pages → `page_0001..` (use synthetic page-count, no real rendering)
  - Read-only target dir → `Err(AppError::PermissionDenied)`
  - Early termination → already-written files remain on disk

### [ ] C6 — feat(tools): pdf_to_images Tauri command
- Module: `src-tauri/src/tools/pdf_to_images/mod.rs`
- `#[tauri::command] async fn convert_pdf_to_images(path: PathBuf, opts: Opts) -> Result<JobResult, AppError>`
- Behaviour: load file → register a `JobId` with `JobRegistry` → call `multitool_core::tools::pdf_to_images::convert` with a progress sink that emits `tool:progress` events keyed by JobId → pipe pages into the writer (target dir derived from input path) → emit `tool:complete` or `tool:error` → return
- **1-line edit to `src-tauri/src/tools/mod.rs::register_commands`** (the registry contract — no other shared-file edits)
- **Tests (`tauri::test`):**
  - Happy path: command returns OK; expected `tool:progress` events emitted in order; `tool:complete` fires
  - Cancellation via `cancel_job` mid-run → command returns `Err(Cancelled)`; partial files exist on disk
  - Bad path → `Err(FileNotFound)`

### [ ] C7 — feat(lib): IPC wrapper for pdf-to-images
**Sets the pattern future IPC wrappers will copy.**
- Module: `src/lib/tools/pdfToImages.ts`
- API: `async function convertPdfToImages(path: string, opts: Opts, { onProgress, signal }: Hooks): Promise<JobResult>`
- Subscribes to `tool:progress` events filtered by JobId; on `AbortSignal` abort calls the `cancel_job` command; on `tool:complete` resolves; on `tool:error` rejects with the typed envelope
- **Tests (Vitest with `@tauri-apps/api` mocked):**
  - Invokes `convert_pdf_to_images` with `{ path, opts }`
  - `onProgress` callback fires for progress events
  - Subscriptions unsubscribed on completion (no leaks)
  - `signal.abort()` → calls `cancel_job` with the JobId
  - Error envelope → rejected promise with typed `{ kind, message }`

### [ ] C8 — feat(tools): pdf-to-images frontend module
- Folder: `src/tools/pdf-to-images/`
  - `index.ts` — `Tool` metadata (`{ id: "pdf-to-images", name, description, category: "convert", route: "/tools/pdf-to-images", component }`)
  - `PdfToImages.tsx` — file picker → form (format radio, DPI input) → Convert button → progress bar → result view ("Open output folder", "Convert another")
  - `types.ts` — TS mirrors of Rust `Opts`, `JobResult`
- Generate shadcn primitives: `pnpm dlx shadcn add button input progress radio-group label` (refine the exact list during impl)
- **1-line edit to `src/tools/registry.ts`** (the registry contract)
- **Tests (Vitest + Testing Library):**
  - Renders with default option values
  - Selecting JPEG + DPI 300 → IPC wrapper called with those args
  - Progress events render in the progress bar
  - Error from IPC → toast renders with `message`
  - Cancel button → `signal.abort()` called

### [ ] C9 — test(e2e): pdf-to-images happy path
- Playwright spec in `tests/e2e/pdf-to-images.spec.ts`
- Mock `src/lib/tools/pdfToImages.ts` at the boundary — return progress events then success
- Flow: open app → dashboard → click "PDF → Images" tile → simulate file pick (mocked) → click Convert → assert success state with "Open output folder" button
- **Tests:** 1 e2e happy path. Failure paths covered at unit level; e2e is intentionally a smoke.

### [ ] C10 — docs: revise adding-a-tool playbook with PDF→Images learnings
**Closes the loop.** After C9 ships:
- Replace each "Shared surfaces" TODO row in [../adding-a-tool.md](../adding-a-tool.md) with the real path + signature (`unique_path`, `convertPdfToImages`, any extracted shared components)
- Add the IPC wrapper pattern (from C7) as a worked example in the playbook
- Resolve or trim the "TODOs — revisit after PDF→Images ships" section at the bottom of the playbook
- Add DECISIONS entries for anything non-obvious that emerged during the build that isn't already recorded

## Test coverage at a glance

| Commit | Rust unit | Rust integration | Vitest | Playwright |
| --- | --- | --- | --- | --- |
| C1 | — | 1 (pdfium smoke) | — | — |
| C2 | 1–2 | — | — | — |
| C3 | ~10 | — | — | — |
| C4 | ~6 | — | — | — |
| C5 | ~4 | — | — | — |
| C6 | — | ~3 (`tauri::test`) | — | — |
| C7 | — | — | ~5 | — |
| C8 | — | — | ~5 | — |
| C9 | — | — | — | 1 |

Coverage gate to watch: **≥80% line cov on `multitool-core/src/tools/pdf_to_images/convert.rs`** — over-invest in C3's unit tests, since every interesting failure mode lives there.

## When this is done

After C10 merges to `feat/pdf-to-images`:
- [ ] Confirm CHANGELOG.md captures the milestone (likely under v0.1.0)
- [ ] Open the PR from `feat/pdf-to-images` back to `master` — full pre-PR checklist green on all 3 OSes; see [../../CLAUDE.md → Per-PR checklist](../../CLAUDE.md)
- [ ] **Delete this file (`docs/tools/pdf-to-images-plan.md`)** — the brief in [pdf-to-images.md](pdf-to-images.md) is the permanent record; the plan is ephemeral
- [ ] Confirm [../adding-a-tool.md](../adding-a-tool.md) is updated per C10
