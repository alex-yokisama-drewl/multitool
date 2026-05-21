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

### [x] C5 — feat(core): pdf-pages output writer
- Module: `multitool-core/src/tools/pdf_to_images/writer.rs`; `PageWriter` re-exported from the tool's `mod.rs`.
- **API shape (decided):** struct + methods, not a stream consumer. `PageWriter::create(target, format, total_pages) -> AppResult<Self>` resolves through `unique_path` and creates the dir eagerly; `write_page(&PageOutput) -> AppResult<()>` writes one file synchronously; `dir() -> &Path` exposes the resolved dir. Picked over an iterator-consumer API so C6's Tauri command can pass `|p| writer.write_page(&p)` straight into convert's existing `on_page` callback — no second adapter layer.
- **Padding (decided):** caller passes `total_pages` up front (`pad_width = max(3, digits(total))`). Rejected the post-hoc-rename alternative since convert.rs already knows the page count internally — C6 will need to surface it via a small refactor (or render the first page then create the writer); cleaner than two-pass renames on disk.
- **JPEG extension:** `.jpg` (de-facto standard; matches `image::ImageFormat::Jpeg::extensions_str()[0]`). PNG → `.png`. Inline matched in `extension_for`.
- **1-based filenames:** `page.index` is 0-based but the on-disk name is 1-based (`page_001` for index 0) — matches `docs/tools/pdf-to-images.md` and ARCHITECTURE §3.3. Documented on `write_page`.
- **Empty-folder pitfall:** `create` eagerly mkdirs, so calling it before a doomed convert (encrypted/empty) would leave an empty folder. Doc-commented as a C6-coordination concern — C6 should defer `PageWriter::create` until at least one page is in hand.
- **Tests (`writer.rs::tests`, 6 total):** 3-page job + 3-digit padding; padding widens to 4 for `total=1000`; JPEG → `.jpg`; collision routes through `unique_path` and leaves the pre-existing folder untouched; early termination via `drop(writer)` leaves the first two files on disk; `#[cfg(unix)]` permission-denied test chmods `0o555` and asserts the `AppError::PermissionDenied` mapping (Windows can't model POSIX write-bits cleanly — note inline; the mapping in `io_to_app_err` is OS-agnostic, just not end-to-end exercised on Windows).

### [x] C6 — feat(tools): pdf_to_images Tauri command
- **Orchestration pushed to core** as `multitool_core::tools::pdf_to_images::run_job(input, opts, cancel, on_progress)`. The shell command is a ~70-line shim that registers the job, runs `run_job` on a `spawn_blocking` thread, wires `on_progress` to `app.emit("tool:progress", …)`, and emits `tool:complete` / `tool:error` after the join. All behavior worth testing lives in core so it runs under `cargo test -p multitool-core` on every CI OS — see "test-lane divergence" below.
- **`PageOutput::total` added** so `run_job`'s `on_page` adapter can lazy-create the writer on the first page (`PageWriter::create(target, format, page.total)`). Side benefit: a doomed convert (encrypted/empty PDF) leaves no empty output folder. C5's "empty-folder pitfall" doc-warning still applies for any caller that bypasses `run_job` and creates a writer directly.
- **`AppError: Clone`** added so `tool:error` events can serialize a borrow-free payload through Tauri's `emit` (`Serialize + Clone` bound).
- **Runtime-generic command:** `convert_pdf_to_images<R: tauri::Runtime>(app: AppHandle<R>, …)`. Defaulting to bare `tauri::AppHandle` failed `tauri::generate_handler!`'s `CommandArg` resolution against the generic `Builder<R>` from `register_commands`.
- **Registry contract honored:** the *only* shared-file edit is the 3-line addition to `src-tauri/src/tools/mod.rs` (`pub mod pdf_to_images;` + the `convert_pdf_to_images` entry in `generate_handler!`).
- **Event shapes** (private to the shell module, intentionally not in core):
  - `tool:progress` → `{ job_id, progress: { page, total } }`
  - `tool:complete` → `{ job_id, result: JobResult }`
  - `tool:error` → `{ job_id, error: AppError }` (relies on the `{ kind, message }` serde impl from C2)
- **Test-lane divergence (plan shift):** dropped the planned `tauri::test` tests in favor of seven core unit tests on `run_job`. **Why:** both CI and lefthook run `cargo test -p multitool-core --all-targets` (Windows getrandom blocker — `DECISIONS.md` → "Workspace split"), so `tauri::test` cases in the shell crate wouldn't gate anything. The seven core tests cover the plan's three required behaviours (happy path with in-order progress + correct files, cancellation mid-run leaves partial output, missing input → `FileNotFound`) plus encrypted-PDF, output-dir collision via `unique_path`, `on_progress`-error propagation, and a `derive_output_dir` unit. The IPC-event contract is verified at the C7 boundary via the Vitest wrapper tests; the Playwright happy-path in C9 closes the end-to-end loop.

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
| C6 | 7 (`run_job` in core) | — | — | — |
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
