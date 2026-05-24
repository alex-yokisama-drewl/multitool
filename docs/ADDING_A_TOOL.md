# Multitool — Adding a Tool

> Playbook for adding a new tool to the registry. The conceptual patterns these steps embody live in [ARCHITECTURE.md](ARCHITECTURE.md); per-tool working docs live in [plans/](plans/). The first tool to follow this playbook end-to-end is **PDF → Images** (commits `c2…c10` on `feat/pdf-to-images`) — every "see X" link below points at the live reference.

## When to read this

Read this when adding a new tool to the registry. Each tool gets a **single ephemeral working doc** at `docs/plans/<TOOL_NAME>.md` — built in two phases (brief → plan), then deleted once the tool ships. Tools are meant to be self-describing in code.

## What "adding a tool" should look like

A new tool is meant to be **two new folders + two import lines + tests**, with no edits to shared shell or routing code. If you find yourself editing a shared file for anything other than the two registry imports (and adjusting the Dashboard test to assert the new tile, which belongs to the registry contract), stop — that's a sign something belongs in a shared surface, and merits an entry in [DECISIONS.md](DECISIONS.md) explaining why.

## Steps

### 1. Write the working doc

`docs/plans/<TOOL_NAME>.md` — a single working doc per tool, built in two phases:

1. **Assignment brief first.** Inputs, options, output naming, edge cases, acceptance. Use the template at the bottom. Confirm with the user before moving to the next phase.
2. **Then expand into a plan.** Extend (or partly replace) the brief with a list of commit-sized tasks. Update progress in-place as commits land — handoffs between sessions read this doc to pick up where the last one stopped.

Architectural choices that emerge mid-build go to [DECISIONS.md](DECISIONS.md), not the plan. **Delete the working doc when the tool ships.**

### 2. Create the Rust modules

A tool spans two folders: a pure-logic module in `multitool-core` and a thin Tauri-aware wrapper in the shell.

- **In `multitool-core`** (`multitool-core/src/tools/<tool_id>/`):
  - `convert.rs` — `pub fn convert(input, opts, on_page, cancel) -> AppResult<Summary>`. No tauri imports. Pure logic always lives here regardless of dep weight — see [DECISIONS.md](DECISIONS.md) → "Heavy deps allowed in `multitool-core`".
  - `job.rs` (or equivalent) — orchestrates the end-to-end run from a file path: file I/O, deriving the output dir via [`multitool_core::fs::unique_path`][unique_path], threading `convert`'s streaming callback into the [`PageWriter`][writer-ref] (or your tool's equivalent writer). The Tauri command in step 3 is a thin shim over this — keep behaviour worth testing here, not in the shell.
  - `mod.rs` — public re-exports kept small (types + entry points only).
- **In the Tauri shell** (`src-tauri/src/tools/<tool_id>/`):
  - `mod.rs` — `#[tauri::command]` entry points. Pattern: register the JobId, run the orchestrator on a `spawn_blocking` thread, wire the streaming callback to `app.emit("tool:progress", …)`, emit `tool:complete` / `tool:error` after the join, return `Result<JobResult, AppError>` so `invoke` itself carries the success/error. Worked example: [`src-tauri/src/tools/pdf_to_images/mod.rs`](../src-tauri/src/tools/pdf_to_images/mod.rs).
- For multi-output tools (1 input → N outputs), use the streaming `on_page` callback pattern — see [DECISIONS.md](DECISIONS.md) → "Streaming `on_page` callback".

[unique_path]: ../src-tauri/multitool-core/src/fs.rs
[writer-ref]: ../src-tauri/multitool-core/src/tools/pdf_to_images/writer.rs

Tests:
- Unit tests on the pure `convert` covering options, edge cases, and every error variant (target ≥80% line cov — see [ARCHITECTURE.md §4](ARCHITECTURE.md#4-testing-strategy)). PDF → Images' `convert.rs` hit 90.5%.
- Unit tests on the orchestrator covering happy path, cancellation mid-run leaving partial output, missing input, typed-error propagation, and output-dir collision through `unique_path`.
- **No shell-side tests.** CI + lefthook both run `cargo test -p multitool-core --all-targets`; the Tauri shell's test exe can't launch on the Windows runner (see [DECISIONS.md](DECISIONS.md) → "Workspace split"). Anything worth testing must live in `multitool-core`.

### 3. Register the Rust commands

- Add `pub mod <tool_id>;` in [../src-tauri/src/tools/mod.rs](../src-tauri/src/tools/mod.rs)
- Append the tool's `#[tauri::command]` functions to the `generate_handler!` call in `register_commands`
- That should be the only edit to a shared Rust file. `generate_handler!` is compile-time-checked, so a mismatch fails `cargo build` before any UI loads.

### 4. Wire IPC through `src/lib/`

All `@tauri-apps/api` calls live in [../src/lib/](../src/lib/). Tool components import wrappers from `src/lib/`, never `@tauri-apps/api` directly. This is the seam Playwright mocks for e2e (step 7).

Per-tool wrapper (one per tool command):
- File: `src/lib/tools/<toolName>.ts`
- Pattern: generate a JobId via `crypto.randomUUID()` internally (callers never see it), subscribe to `tool:progress` filtered by JobId, await `invoke<JobResult>("convert_<tool>", { jobId, path, opts })` so the invoke promise carries completion/error directly. Thread an `AbortSignal` through `cancel_job` on abort. Use `try { … } finally { unlisten(); }` so the listener can't leak on the error path.
- Wire shape: `JobResult` keeps Rust snake_case (`output_dir`, `page_count`, `duration_ms`) — a thin shape adapter, no renames to drift on.
- Worked example: [`src/lib/tools/pdfToImages.ts`](../src/lib/tools/pdfToImages.ts) + tests in [`pdfToImages.test.ts`](../src/lib/tools/pdfToImages.test.ts).

System-level wrappers (file picker, reveal-in-folder) live in [`src/lib/system.ts`](../src/lib/system.ts) — extend that file rather than importing `@tauri-apps/plugin-*` from components.

### 5. Create the frontend module

- Folder: `src/tools/<tool-id>/`
- Files:
  - `index.ts` — exports `Tool` metadata (`{ id, name, description, category, color, route, component }`). `category` is the file type the tool handles (`"pdf"`, `"image"`, …) — tiles on the dashboard are grouped by category. If your tool fits an existing category, no shared edit is needed; introducing a brand-new category means extending the `ToolCategory` union + `toolCategories` list in [../src/tools/registry.ts](../src/tools/registry.ts) (a deliberate, narrow shared edit, not a registry-pattern violation). `color` is a soft tile-background token from the `TileColor` union in the same file; the actual color values live as `--tile-<name>` / `--tile-<name>-fg` pairs in [../src/app/globals.css](../src/app/globals.css). Repeats across tools are fine; pick one that's visually distinct from neighbours in the same category. Add a new color = add the CSS-var pair + extend the union.
  - `<ToolName>.tsx` — presentational; all IPC goes through wrappers in `src/lib/`. State machine pattern recommended: `idle → picked → running → done | error` with the error arm preserving the picked file so the user can retry without re-picking. Worked example: [`src/tools/pdf-to-images/PdfToImages.tsx`](../src/tools/pdf-to-images/PdfToImages.tsx).
  - `types.ts` — thin re-exports of the wrapper's types so the tool folder is self-describing.
- Use primitives from [`src/components/ui/`](../src/components/ui/) (shadcn) and shared components in [`src/components/`](../src/components/) — don't reach into another tool's folder.
- shadcn primitives generate on demand: `pnpm dlx shadcn add <name>`. After generating, the project gates (especially `prefer-nullish-coalescing`) may require a one-line tweak in the generated file — expected, not a code-smell.

### 6. Register the frontend tool

- Add the import + array entry in [../src/tools/registry.ts](../src/tools/registry.ts)
- Update [../src/app/Dashboard.test.tsx](../src/app/Dashboard.test.tsx) to assert the new tile (the test belongs to the registry contract).
- That should be the only edits to shared frontend files.

### 7. Add tests

| Layer | Runner | What to cover |
| --- | --- | --- |
| Rust pure (`convert.rs`) | `cargo test -p multitool-core` | Inputs, options, edge cases, every error variant. ≥80% line cov. |
| Rust orchestrator (`job.rs`) | `cargo test -p multitool-core` | Happy path, cancellation, missing input, typed errors, `unique_path` collision. |
| TS IPC wrapper | Vitest, `@tauri-apps/api/{core,event}` mocked via `vi.hoisted` | Invokes the right command, progress filtered by JobId, listener unsubscribed on both success + error, AbortSignal abort → `cancel_job`, error envelope round-trip. |
| React component | Vitest + Testing Library, `@/lib/*` mocked | Defaults render, options forwarded, progress text renders, error envelope renders, Cancel aborts the signal. |
| E2E happy path | Playwright against `pnpm dev` | One smoke: dashboard → tile → form → success. Failure paths stay at the unit level. |

E2E mocking: the wrappers under `src/lib/` get swapped for `tests/e2e/mocks/*.ts` via a Vite alias gated on `VITE_E2E=true` (set on `playwright.config.ts → webServer.env`; alias logic in [`vite.config.ts`](../vite.config.ts)). New tools that need different e2e mocks add a sibling file under `tests/e2e/mocks/` and extend the alias map. Mocks are typed against the real wrapper signatures so drift surfaces as a tsc error.

### 8. Run the pre-PR checklist

See [../CLAUDE.md → Per-PR checklist](../CLAUDE.md). Short version: fmt → clippy (workspace) → `cargo test -p multitool-core --all-targets` → pnpm lint/typecheck/test → `pnpm tauri build --no-bundle` → `pnpm test:e2e`. CI is the cross-OS truth: linux-local gates can pass while Windows/macOS regress on native-dep code, so any "path-shaped" commit (new native deps, build scripts, FS code) deserves a CI sweep before stacking more work on top.

## Shared surfaces

A new tool should consume from these, not duplicate.

| Surface | Location | Notes |
| --- | --- | --- |
| Error type | `multitool_core::error::AppError` | Variants: `FileNotFound`, `PermissionDenied`, `UnsupportedFormat`, `ProcessingFailed { detail }`, `Encrypted`, `Cancelled`. Serialized as `{ kind, message }` to the webview. Add a new variant only when the UI branches on it; otherwise use `ProcessingFailed { detail }`. |
| Image decode (EXIF-aware) | [`multitool_core::image`](../src-tauri/multitool-core/src/image.rs) | `decode_oriented(source_ext, bytes) -> AppResult<DynamicImage>` applies any EXIF orientation tag the decoder exposes; falls back to extension-based format detection when bytes-sniffing is inconclusive (TGA has no magic). `image_to_app_err(err)` is the companion variant mapper. Both `images_to_pdf` and `image_format_converter` route through this. |
| Path + duplicate-name helpers (pure) | [`multitool-core/src/fs.rs`](../src-tauri/multitool-core/src/fs.rs) | `unique_path(target) -> PathBuf` resolves `foo` → `foo (1)` on collision; not race-safe (single-thread only). Multi-dot stems: suffix lands before the trailing extension (`foo.tar.gz` → `foo.tar (1).gz`). |
| Tauri-aware path helpers | [../src-tauri/src/fs/](../src-tauri/src/fs/) | Reserved for resolution that genuinely needs Tauri APIs (e.g. app-data dir). Empty so far. |
| Cancellation | `multitool_core::ipc::JobRegistry` + the `cancel_job` command | `registry.register(jobId)` returns a `CancellationToken`; check it between work units. `cancel_job(jobId)` triggers it and is idempotent. |
| Tauri shim helper | [`crate::ipc::run_streaming_job`](../src-tauri/src/ipc/streaming_job.rs) | Wraps the register → `spawn_blocking` → emit-progress → unregister → emit-complete/error dance every tool's `#[tauri::command]` shim used to repeat. Each shim passes a `run` closure that calls its `run_job` with the supplied cancel + emit. Tools' `run_job` should take args in `(inputs, opts, cancel, on_progress)` order to keep the closure trivial. |
| Pdfium singleton | `multitool_core::pdfium::instance()` | Process-wide; never call `Pdfium::new` directly (would panic on the second call). See [DECISIONS.md](DECISIONS.md) → "Pdfium is a process-wide singleton". |
| TS IPC wrappers | [../src/lib/tools/](../src/lib/tools/) | One file per tool: `<toolName>.ts`. Pattern + worked example in step 4. |
| System-level OS wrappers | [`src/lib/system.ts`](../src/lib/system.ts) | `pickPdfFile()`, `revealInFolder(path)`, `pickImageFiles()` / `pickConvertibleImages()`, `imageAssetUrl()`, `allowImagePreview()`. Extend here when a tool needs picker / dialog / opener; don't import `@tauri-apps/plugin-*` from components. |
| Path / filename utilities | [`src/lib/utils.ts`](../src/lib/utils.ts) | `cn()` for class merging, `fileName(path)` + `fileStem(path)` for display. `fileStem` mirrors Rust's `Path::file_stem` (multi-dot stems keep all but the last extension). |
| Tauri plugins baseline | `tauri-plugin-dialog`, `tauri-plugin-opener` | Already registered in [../src-tauri/src/lib.rs](../src-tauri/src/lib.rs); capabilities granted in [../src-tauri/capabilities/default.json](../src-tauri/capabilities/default.json). Add a new plugin only with a written DECISIONS entry and the narrowest capability grant. |
| shadcn UI primitives | [../src/components/ui/](../src/components/ui/) | `button`, `input`, `label`, `progress`, `radio-group` ship today. Generate more on demand via `pnpm dlx shadcn add <name>`. |
| Shared components (FilePicker, JobProgress, …) | [../src/components/](../src/components/) | None yet — bias toward inlining in the tool folder until the same shape shows up in a second tool. |
| Vitest test setup | [../tests/setup.ts](../tests/setup.ts) | Wires jest-dom + Testing Library's `afterEach(cleanup)` (needed because vitest runs `globals: false`). |

When a tool needs something not in this list, decide: (a) one-off inside the tool's folder, or (b) extract to a shared surface. Bias toward (a) until you see the pattern twice — see [../CLAUDE.md → Scope discipline](../CLAUDE.md).

## Per-tool brief template

Save as `docs/plans/<TOOL_NAME>.md`. Ephemeral — extended into a task plan during build, deleted when the tool ships. Keep it short and implementation-flavored.

```markdown
# Tool: <Name>

## Summary
One sentence.

## Inputs
- ...

## Options
| Option | Type | Default | Notes |
| --- | --- | --- | --- |

## Output
- Location:
- Naming:
- Duplicate handling: per [../ARCHITECTURE.md §3.3](../ARCHITECTURE.md#33-file-io-conventions)

## UX flow
Dashboard → <Tool> → Result. Note non-default flows + cancellation behavior.

## Edge cases
- ...

## Acceptance
- [ ] ...
```
