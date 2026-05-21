# PDF → Images — Build Plan

> **Working doc — ephemeral.** Tracks the build sequence for the PDF→Images tool. The permanent brief is [pdf-to-images.md](pdf-to-images.md); architectural decisions live in [../../DECISIONS.md](../../DECISIONS.md). When all commits ship, follow the "When this is done" checklist at the bottom and **delete this file**.

## Working agreement (for the agent executing this plan)

- **Mark commits `[x]` as soon as they merge.** Don't batch.
- **Edit this file when scope shifts.** If a commit splits, merges, gets reordered, or is dropped — update the list. This doc should always reflect the actual path forward, not the original plan. Stale steps are worse than no steps.
- **Architectural choices go to [../../DECISIONS.md](../../DECISIONS.md), not here.** This file is the checklist; the why-history lives in DECISIONS.
- **One open question at a time.** If you hit a fork that isn't already resolved in DECISIONS, surface it in chat with the user before committing — same protocol as the original planning session.
- **Per-commit gates apply locally:** fmt, clippy, pnpm lint/typecheck/test, and `pnpm tauri build --no-bundle` must be green before the commit lands. See [../../CLAUDE.md → Per-PR checklist](../../CLAUDE.md). Lefthook enforces a subset on commit; the rest is your responsibility.
- **Cross-OS CI check is opt-in mid-stream.** Local gates are linux-only. After any commit that could plausibly break Windows/macOS (new native deps, build scripts, FS code, anything path-shaped), ask the user whether to push `feat/pdf-to-images` and watch CI before continuing — don't gate on it by default. Pure-Rust pure-logic commits can usually wait for the PR-time CI sweep.

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

### [x] C2 — feat(core): AppError::Encrypted variant
- `Encrypted` variant (no payload) added to `multitool_core::error::AppError`; `kind()` returns `"Encrypted"`, `Display` explains the Phase-1 limitation.
- No frontend TS mirror exists yet — added when C7 introduces the IPC wrapper.
- Serialization round-trip test added alongside existing AppError tests.

### [x] C3 — feat(core): pdf-to-images pure conversion
- Module: `multitool-core/src/tools/pdf_to_images/{mod.rs, convert.rs}` with the planned public API (`Opts`, `Format`, `PageOutput`, `JobSummary`, `convert`).
- **DPI contract:** silently clamped to `[DPI_MIN, DPI_MAX]` = `[72, 600]`. Documented on `Opts`; tested at both bounds.
- Fixtures (≤ 20 KB each): `single-page.pdf`, `encrypted.pdf` (gs-produced), `corrupt.pdf`, `zero-page.pdf`. Generator extended in `scripts/gen_pdf_fixture.py`.
- **Pdfium singleton:** `pdfium::instance()` (OnceLock + Mutex) replaces the per-call `bind_to_library`; rationale + future-tool guidance recorded in [../../DECISIONS.md](../../DECISIONS.md) → "Pdfium is a process-wide singleton".
- **Tests:** 11 unit tests in `convert.rs` covering PNG/JPEG magic, DPI scaling, encrypted/corrupt/zero-page errors, cancellation (before any page + between pages), `on_page` Err propagation, DPI clamp at both bounds. `cargo llvm-cov` reports **90.54% line coverage** on `convert.rs`.

### [x] C4 — feat(core): unique_path helper for duplicate-name policy
- Module: `multitool-core/src/fs.rs`; `pub fn unique_path(target: &Path) -> std::io::Result<PathBuf>` registered in `lib.rs`.
- **File-vs-dir branch** keyed off `Path::extension()`: extension present → suffix goes before the extension (`file_stem` is the base); no extension → suffix appended to the full `file_name`. Treats directories and extensionless files identically — both append-to-full-name — which keeps the policy declarative without a stat call.
- **Multi-dot stems** (`foo.tar.gz`): suffix lands before the final `.gz` (→ `foo.tar (1).gz`) because `Path::extension()` only returns the trailing segment. Matches Finder / Windows Explorer; rationale recorded as an inline comment in the test.
- **Not race-safe** — documented on the fn; single-threaded use only.
- `tempfile = "3"` added to `[dev-dependencies]` for the tests.
- **Tests:** all 6 from the plan land in `fs.rs::tests` (free target, file collision, double collision, directory collision, multi-dot stem, no-extension). All pass; clippy clean with `unwrap_used`/`expect_used` denial intact (only the unwrap-allowed test block uses `.unwrap()`).

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
