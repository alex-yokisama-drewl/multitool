# Multitool — Architecture

How the app is built. The operating agreement for AI-assisted contributions lives in [../CLAUDE.md](../CLAUDE.md); concrete decision history (including the "why we chose X" entries that don't fit here) lives in [DECISIONS.md](DECISIONS.md); forward-looking plans and ideas live in [plans/BACKLOG.md](plans/BACKLOG.md).

## 1. Tech Stack

| Layer             | Choice                            | Rationale                                                                                              |
| ----------------- | --------------------------------- | ------------------------------------------------------------------------------------------------------ |
| Runtime shell     | **Tauri 2.x**                     | ~10× smaller bundles than Electron, native-speed file/image processing, no shipped JS runtime         |
| Backend           | **Rust**                          | Native performance for media work, strong type system, easy async via Tokio                            |
| Frontend          | **React 18 + TypeScript + Vite**  | Best-documented Tauri pairing; mature ecosystem                                                        |
| UI components     | **Tailwind CSS + shadcn/ui**      | Polished, minimal components out of the box; no heavy component library                                |
| State             | **Zustand**                       | Lightweight, no boilerplate, good fit for a small app                                                  |
| Routing           | **React Router** (hash mode)      | Single-window app; hash routing avoids Tauri custom protocol quirks                                    |
| Image processing  | **`image` crate**                 | De facto standard for Rust image I/O                                                                   |
| PDF rendering     | **`pdfium-render`**               | Mature PDFium bindings; ships a small native binary (~5MB per platform)                                |
| PDF creation      | **`printpdf`**                    | Pure Rust, sufficient for image-to-PDF                                                                 |
| Package manager   | **pnpm 9**                        | No npm/yarn lockfiles; matches CI                                                                      |

Frontend alternative considered: Svelte (smaller bundle), not chosen because the shadcn/ui ecosystem is React-first and saves us building primitives ourselves.

## 2. Crate / Workspace Layout

The cargo workspace lives at `src-tauri/Cargo.toml` (no `default-members`). Two crates:

- **`multitool`** — the Tauri shell. Entry points (`lib.rs`/`main.rs`), `ipc/` for cancellation glue + the streaming-job shim helper, `tools/` for per-tool `#[tauri::command]` wrappers, `build.rs` that downloads and bundles the pdfium binary as a Tauri resource.
- **`multitool-core`** — pure-logic rlib, no `tauri` dep. `tools/` for per-tool conversion + orchestration logic, plus `error.rs`, `ipc.rs` (`JobId` / `JobRegistry`), `fs.rs` (pure path helpers), `pdfium.rs` (process-wide singleton), and its own `build.rs` that downloads + pins the pdfium binary for dev/test.

`multitool-core` exists so processing logic stays pure-function and testable without spinning up Tauri (see §3.1) and to keep the Tauri runtime out of the test exe — the shell's test exe doesn't launch on the Windows CI runner ([DECISIONS.md](DECISIONS.md) → "Workspace split").

## 3. Architecture Patterns

### 3.1 Tool Registry Pattern

Each tool is a self-contained module on both sides of the IPC boundary: a Rust folder under `multitool-core/src/tools/<tool_id>/` for pure logic (`convert.rs`, `job.rs`) plus a thin `#[tauri::command]` shim under `src-tauri/src/tools/<tool_id>/`, and a TS folder under `src/tools/<tool-id>/` with registry metadata (`index.ts`), the presentational component, and a `types.ts` mirroring the Rust types.

A central `src/tools/registry.ts` imports each tool's metadata and exposes the list to the dashboard. On the Rust side, `src-tauri/src/tools/mod.rs::register_commands` aggregates `generate_handler!` invocations and is called once from `lib.rs::run()`. **Adding a new tool = one folder per side + one import line per registry, no edits to shared shell/routing code.** Step-by-step playbook: [ADDING_A_TOOL.md](ADDING_A_TOOL.md).

### 3.2 Process Model

- **Webview (UI):** rendering only; no file I/O, no heavy computation. All `@tauri-apps/api` calls go through wrappers in `src/lib/` — components stay presentational and that boundary is where Playwright mocks the Tauri layer.
- **Rust main:** Tauri commands dispatch work to Tokio tasks.
- **Worker tasks:** long-running operations run async; progress streams to UI via Tauri events (`tool:progress`, `tool:complete`, `tool:error`).
- **Cancellation:** every long-running command takes a cancellation token tied to a `JobId`. The token registry lives in `multitool_core::ipc::JobRegistry`; UI can cancel mid-operation via the `cancel_job` Tauri command.
- **Concurrency:** one active job per tool by default; user can navigate away while it runs.

### 3.3 File I/O Conventions

- **Default output location:** same directory as the input.
- **Naming:** `{stem}_{tool_suffix}.{ext}` (e.g., `report.pdf` → folder `report_pages/` containing `page_001.png`, …).
- **Duplicate handling:** if the target exists, append ` (1)`, ` (2)`, … until a free name is found. Never overwrite silently.

### 3.4 Error Handling

- All Rust commands return `Result<T, AppError>` with typed variants (`FileNotFound`, `PermissionDenied`, `UnsupportedFormat`, `ProcessingFailed`, `Encrypted`, `Cancelled`)
- `AppError` serializes as `{ kind, message }` so the webview can branch on `kind`
- UI surfaces errors as non-blocking toasts; retry is offered where applicable
- No `unwrap()` or `expect()` in non-test Rust code; `clippy::unwrap_used` denied at the crate level (`cfg_attr(not(test), ...)` so unit tests can still use `.unwrap()` freely)

## 4. Testing Strategy

| Layer                          | Tool                                          | Target                                          |
| ------------------------------ | --------------------------------------------- | ----------------------------------------------- |
| Rust pure logic                | `cargo test -p multitool-core` + fixtures     | ≥80% line coverage on `tools/*/convert.rs`      |
| TS units (registry, utils)     | Vitest                                        | Critical paths covered                          |
| React components               | Vitest + Testing Library                      | Each tool's UI smoke-tested                     |
| End-to-end                     | Playwright against `pnpm dev`                 | Two flows: PDF→images, images→PDF, happy path  |

Processing functions are written as pure functions so they can be tested without spinning up Tauri. Coverage runs via `cargo-llvm-cov` (works on all three CI OSes; tarpaulin is Linux-only).

The Tauri shell crate has no dedicated test lane. CI and lefthook both run `cargo test -p multitool-core --all-targets`, which deliberately excludes the shell — the shell's test exe doesn't launch on the Windows CI runner (see [DECISIONS.md](DECISIONS.md) → "Workspace split"). Shell modules are kept thin enough (orchestration delegates into `multitool-core`; presentational logic lives in React) that the IPC contract is covered by the Vitest wrapper tests at the `src/lib/` boundary and the Playwright happy-path lane.

Playwright drives the Vite dev server with the Tauri IPC layer mocked at the `src/lib/` wrapper boundary. Tauri's own WebDriver path (`tauri-driver`) is WebdriverIO-only and can be added as a second e2e lane later if needed.

## 5. CI / Release Pipeline

- **`.github/workflows/ci.yml`** — runs on PR across `ubuntu-latest`, `macos-latest`, `windows-latest` (`fail-fast: false`). Steps mirror the [../CLAUDE.md](../CLAUDE.md) per-PR checklist: fmt → clippy (workspace) → `cargo test -p multitool-core --all-targets` → pnpm lint → typecheck → vitest → `pnpm tauri build --no-bundle`. No push-to-master trigger — squash-merges land the same diff CI already validated on the PR.
- **`.github/workflows/release.yml`** — fires on `v*` tag push, uses the same matrix, builds per-platform artifacts via `tauri-apps/tauri-action`, and attaches them to a **draft** GitHub Release (`releaseDraft: true`). Tags containing `-` (e.g. `v0.1.0-scaffold`) are auto-marked prerelease. No macOS signing/notarization.
- **Branch protection on `master`** (classic) — requires `linux` / `macos` / `windows` contexts and linear history; force-push and deletion blocked. `enforce_admins: false` so the owner can override when needed.

`-p multitool-core` on `cargo test` is mandatory: the Tauri shell's test exe fails to launch on the Windows CI runner. See [DECISIONS.md](DECISIONS.md) → "Workspace split" and "No default-members".
