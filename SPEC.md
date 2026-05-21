# Multitool — Technical Specification

**Status:** Draft v0.1
**Working name:** multitool

## 1. Overview

Cross-platform desktop application providing an offline, all-in-one alternative to common online conversion tools (image format conversion, PDF assembly, audio trimming, etc.). Phase 1 ships two converters; the architecture is designed so additional tools can be added as self-contained modules without touching shared code.

## 2. Goals

- Small bundle size, fast startup, low memory footprint
- Fully offline; no network calls at runtime
- Minimal-friction UX: pick input, get output, done
- Cross-platform builds: Linux, macOS, Windows (build matrix exercised for learning purposes; primary daily-use targets are Linux and Windows)
- Built to high engineering standards (clean architecture, test coverage, conventional commits, semantic versioning, CI)

**Scope note:** this is a learning project. It is not intended for public distribution. The author uses it personally on Linux and Windows. macOS is supported through the CI build matrix but is not a daily-driver target — accordingly, no macOS code signing, notarization, or publishing pipeline is planned.

## 3. Non-Goals (Phase 1)

- Cloud sync, user accounts, telemetry
- Third-party plugin system (modularity is internal only)
- Advanced in-tool editing (e.g., reordering pages mid-flow)
- Auto-updates (deferred to a later phase)

## 4. Tech Stack

| Layer | Choice | Rationale |
|---|---|---|
| Runtime shell | **Tauri 2.x** | ~10× smaller bundles than Electron, native-speed file/image processing, no shipped JS runtime |
| Backend | **Rust** | Native performance for media work, strong type system, easy async via Tokio |
| Frontend | **React 18 + TypeScript + Vite** | Best-documented Tauri pairing; mature ecosystem |
| UI components | **Tailwind CSS + shadcn/ui** | Polished, minimal components out of the box; no heavy component library |
| State | **Zustand** | Lightweight, no boilerplate, good fit for a small app |
| Routing | **React Router** (hash mode) | Single-window app; hash routing avoids Tauri custom protocol quirks |
| Image processing | **`image` crate** | De facto standard for Rust image I/O |
| PDF rendering | **`pdfium-render`** | Mature PDFium bindings; ships a small native binary (~5MB per platform) |
| PDF creation | **`printpdf`** | Pure Rust, sufficient for image-to-PDF |

**Frontend alternative considered:** Svelte (smaller bundle), not chosen because the shadcn/ui ecosystem is React-first and saves us building primitives ourselves.

## 5. Architecture

### 5.1 Tool Registry Pattern

Each tool is a self-contained module on both sides of the IPC boundary:

```
src/tools/
  pdf-to-images/
    index.ts          # registry metadata: { id, name, category, icon, route, component }
    PdfToImages.tsx   # UI component
    types.ts          # shared input/output contracts (mirrors Rust types)
  images-to-pdf/
    ...

src-tauri/src/tools/
  pdf_to_images/
    mod.rs            # exposes #[tauri::command] entry points
    convert.rs        # pure processing logic, testable without Tauri
  images_to_pdf/
    ...
```

A central `src/tools/registry.ts` imports each tool's metadata and exposes the list to the dashboard. **Adding a new tool = adding one folder on each side and one import line in each registry.** No edits to shared files.

### 5.2 Process Model

- **Webview (UI):** rendering only; no file I/O, no heavy computation
- **Rust main:** Tauri commands dispatch work to Tokio tasks
- **Worker tasks:** long-running operations run async; progress streams to UI via Tauri events (`tool:progress`, `tool:complete`, `tool:error`)
- **Cancellation:** every long-running command takes a cancellation token tied to a job ID; UI can cancel mid-operation
- **Concurrency:** one active job per tool by default; user can navigate away while it runs

### 5.3 File I/O Conventions

- **Default output location:** same directory as the input
- **Naming:** `{stem}_{tool_suffix}.{ext}` (e.g., `report.pdf` → folder `report_pages/` containing `page_001.png`, ...)
- **Duplicate handling:** if the target exists, append ` (1)`, ` (2)`, ... until a free name is found. Never overwrite silently.
- **Output override:** user may pick a different destination per operation; this is a secondary UI affordance, not the default path

### 5.4 Error Handling

- All Rust commands return `Result<T, AppError>` with typed variants (`FileNotFound`, `PermissionDenied`, `UnsupportedFormat`, `ProcessingFailed`, `Cancelled`)
- UI surfaces errors as non-blocking toasts; retry is offered where applicable
- No `unwrap()` or `expect()` in non-test Rust code; `clippy::unwrap_used` enforced

## 6. UX Principles

- Single window; no modal dialogs in primary flows
- Navigation: **Dashboard → Tool → Result**, with persistent back button
- Minimal chrome; content-first
- Keyboard shortcuts: `Esc` to go back, `Ctrl/Cmd+O` to open file picker
- **Drag-and-drop input:** Phase 1 stretch goal — implement if time permits, otherwise slip to Phase 2

## 7. Phase 1 Scope

### 7.1 Tool: PDF → Images

- **Input:** one PDF file (via picker or drag-and-drop)
- **Output:** one image per page, in a subfolder named `{pdf_stem}_pages/`, in the input directory
- **Options:** output format (PNG default, JPEG), DPI (default 150)
- **Progress:** per-page progress reported to UI

### 7.2 Tool: Images → PDF

- **Input:** one or more image files (PNG, JPEG, WebP), selected via picker (and drag-and-drop if 0.1.0 stretch goal lands)
- **Staging area UX:** selected images appear as a reorderable thumbnail list before generation
  - **Reorder:** drag-and-drop within the list to change page order (default order is filename ascending on initial selection)
  - **Add more:** an explicit "Add images" affordance is always available; opens the picker again and appends to the existing list. Supports images from multiple folders in a single PDF.
  - **Remove:** per-thumbnail remove control
  - Nothing is written to disk until the user explicitly clicks "Create PDF"
- **Output:** one PDF named `{first_image_stem}.pdf` in the directory of the first image (duplicate-handling rules from §5.3 apply)
- **Options:** page size (auto-fit-to-image default, A4, Letter)
- **Progress:** per-image progress reported to UI

## 8. Quality Standards

### 8.1 Testing

| Layer | Tool | Target |
|---|---|---|
| Rust processing logic | `cargo test` with fixture files | ≥80% line coverage on `tools/*/convert.rs` |
| Rust commands (integration) | `cargo test` with `tauri::test` harness | All command paths exercised |
| TS units (registry, utils) | Vitest | Critical paths covered |
| React components | Vitest + Testing Library | Each tool's UI smoke-tested |
| End-to-end | Playwright | Two flows: PDF→images, images→PDF, happy path each |

Processing functions are written as pure functions (`fn(input: &[u8], opts: &Opts) -> Result<Vec<u8>>`) so they can be tested without spinning up Tauri.

### 8.2 Git & Versioning

- **Branch model:** trunk-based; short-lived feature branches, PRs into `master`
- **Commits:** Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `chore:`, `docs:`)
- **Versioning:** Semantic Versioning; tags on `master` (`v0.1.0`, ...)
- **Changelog:** auto-generated from conventional commits (e.g., `git-cliff`)
- **PR hygiene:** every PR has a description, passes CI, and is reviewed before merge (self-review for solo work)

### 8.3 CI/CD (GitHub Actions)

- On PR: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `pnpm lint`, `pnpm test`, type-check, build verification on Linux/macOS/Windows
- On tag push: build release artifacts per platform, attach to GitHub Release

### 8.4 Code Quality

- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `clippy::unwrap_used` denied
- TypeScript: ESLint + Prettier, `strict: true` in `tsconfig`
- Pre-commit hooks via `lefthook`: fmt + lint on staged files

## 9. Repository Layout

```
multitool/
├── src/                       # Frontend (React + TS)
│   ├── app/                   # App shell, routing, layout
│   ├── components/            # Shared UI (Button, Toast, ...)
│   ├── tools/                 # One folder per tool + registry.ts
│   ├── lib/                   # Tauri IPC wrappers, utilities
│   └── main.tsx
├── src-tauri/                 # Backend (Rust)
│   ├── src/
│   │   ├── tools/             # One module per tool + registry
│   │   ├── ipc/               # Command + event plumbing
│   │   ├── fs/                # Path resolution, duplicate handling
│   │   └── main.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── tests/                     # Playwright e2e
├── .github/workflows/         # CI
├── SPEC.md                    # This file
├── README.md
├── CHANGELOG.md
└── package.json
```

## 10. Roadmap

The project intentionally stays in the `0.x` pre-release range. There is no planned `1.0.0` — bumping to 1.0 would imply a stable, publishable product, which is out of scope for this learning project. Future major changes can be revisited if intent ever shifts.

| Version | Scope |
|---|---|
| **0.1.0** | Project scaffold, dashboard shell, tool registry, both Phase 1 tools (with staging-area UX for Images→PDF), full test suite, CI on all three OSes |
| **0.2.0** | Drag-and-drop input on dashboard/tools, output-location override UI |
| **0.3.0+** | Additional tools (image format conversion, audio trim, ...) — each as a new module under the registry |

## 11. Open Questions

1. **App icon & branding** — placeholder is acceptable; not blocking any milestone
2. **Auto-updates** — not planned (no distribution channel); can revisit if intent changes
3. **macOS code signing / notarization** — explicitly out of scope; the macOS CI build verifies the project compiles and runs there, but no distributable artifact is produced
4. **Telemetry** — explicitly off; if ever added, must be opt-in only
