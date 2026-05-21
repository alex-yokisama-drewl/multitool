# Multitool — Assignment

> **Working doc.** Defines what the app does and how to know it's done — the PM-style brief, intentionally free of implementation detail. Architecture and tech-stack decisions live in [ARCHITECTURE.md](ARCHITECTURE.md); the working agreement for AI-assisted contributions lives in [CLAUDE.md](CLAUDE.md); decision history lives in [DECISIONS.md](DECISIONS.md). This file gets retired once the Phase 1 scope below ships.

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

## 4. UX Principles

- Single window; no modal dialogs in primary flows
- Navigation: **Dashboard → Tool → Result**, with persistent back button
- Minimal chrome; content-first
- Keyboard shortcuts: `Esc` to go back, `Ctrl/Cmd+O` to open file picker
- **Drag-and-drop input:** Phase 1 stretch goal — implement if time permits, otherwise slip to Phase 2

## 5. Phase 1 Scope

### 5.1 Tool: PDF → Images

- **Input:** one PDF file (via picker or drag-and-drop)
- **Output:** one image per page, in a subfolder named `{pdf_stem}_pages/`, in the input directory
- **Options:** output format (PNG default, JPEG), DPI (default 150)
- **Progress:** per-page progress reported to UI

### 5.2 Tool: Images → PDF

- **Input:** one or more image files (PNG, JPEG, WebP), selected via picker (and drag-and-drop if 0.1.0 stretch goal lands)
- **Staging area UX:** selected images appear as a reorderable thumbnail list before generation
  - **Reorder:** drag-and-drop within the list to change page order (default order is filename ascending on initial selection)
  - **Add more:** an explicit "Add images" affordance is always available; opens the picker again and appends to the existing list. Supports images from multiple folders in a single PDF.
  - **Remove:** per-thumbnail remove control
  - Nothing is written to disk until the user explicitly clicks "Create PDF"
- **Output:** one PDF named `{first_image_stem}.pdf` in the directory of the first image (duplicate-handling rules from [ARCHITECTURE.md §3.3](ARCHITECTURE.md#33-file-io-conventions) apply)
- **Options:** page size (auto-fit-to-image default, A4, Letter)
- **Progress:** per-image progress reported to UI

## 6. Roadmap

The project intentionally stays in the `0.x` pre-release range. There is no planned `1.0.0` — bumping to 1.0 would imply a stable, publishable product, which is out of scope for this learning project. Future major changes can be revisited if intent ever shifts.

| Version    | Scope                                                                                                                                                  |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **0.1.0**  | Project scaffold, dashboard shell, tool registry, both Phase 1 tools (with staging-area UX for Images→PDF), full test suite, CI on all three OSes      |
| **0.2.0**  | Drag-and-drop input on dashboard/tools, output-location override UI, dashboard tile redesign (square tiles, per-tool color set at registration, category-grouped sections with separator + label) |
| **0.3.0+** | Additional tools (image format conversion, audio trim, ...) — each as a new module under the registry                                                  |

## 7. Nice-to-haves & follow-ups

Not committed to a milestone; record so they don't get lost.

- **Paste-from-clipboard for image inputs.** Any tool that accepts images should allow pasting (Ctrl/Cmd+V) directly — screenshots in particular. Likely a shared input affordance rather than per-tool.
- **Image format converter.** A future tool to convert between image formats. Details TBD when it gets built.
- **Doc consolidation.** Once the dashboard redesign settles and the only foreseeable future work is adding tools, fold [DECISIONS.md](DECISIONS.md) into [ARCHITECTURE.md](ARCHITECTURE.md) as a single technical doc.

## 8. Open Questions

1. **App icon & branding** — placeholder is acceptable; not blocking any milestone
2. **Auto-updates** — not planned (no distribution channel); can revisit if intent changes
3. **macOS code signing / notarization** — explicitly out of scope; the macOS CI build verifies the project compiles and runs there, but no distributable artifact is produced
4. **Telemetry** — explicitly off; if ever added, must be opt-in only
