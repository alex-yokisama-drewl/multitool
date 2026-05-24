# multitool

[![CI](https://github.com/alex-yokisama-drewl/multitool/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/alex-yokisama-drewl/multitool/actions/workflows/ci.yml)

Cross-platform offline desktop multitool — image / PDF / etc. conversions that normally need a web app, run natively without the round trip. Tauri 2 + React + TypeScript on the frontend; Rust + Tokio on the backend. **Learning project** — not aimed at public distribution; intentionally stays in the `0.x` pre-release range.

## Project docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — tech stack and architecture patterns
- [docs/DECISIONS.md](docs/DECISIONS.md) — running log of noteworthy choices and caveats
- [docs/ADDING_A_TOOL.md](docs/ADDING_A_TOOL.md) — playbook for adding a new tool to the registry
- [docs/plans/BACKLOG.md](docs/plans/BACKLOG.md) — plans and ideas not yet committed to a milestone
- [CLAUDE.md](CLAUDE.md) — working agreement for AI-assisted contributions; loaded into every Claude Code session
- [CHANGELOG.md](CHANGELOG.md) — per-release notes (generated from conventional commits)

## Quickstart

Prereqs: Rust stable (`rustup install stable`), Node 20+, pnpm 9, and the Tauri 2 [system dependencies](https://tauri.app/start/prerequisites/) for your platform. On Linux that's `webkit2gtk-4.1`, `libsoup-3.0`, `libayatana-appindicator3`, `librsvg2`, `libxdo` and friends — the full apt list lives in [.github/workflows/ci.yml](.github/workflows/ci.yml).

```bash
pnpm install            # JS deps + register lefthook hooks
pnpm tauri dev          # launch dev server + Tauri window
```

Compile a release binary without producing platform installers:

```bash
pnpm tauri build --no-bundle
```

The full test/lint/coverage command reference is in [CLAUDE.md → Commands cheat sheet](CLAUDE.md#commands-cheat-sheet).

## Project layout

- `src/` — React frontend; `src/tools/` is the tool registry ([ARCHITECTURE §3.1](docs/ARCHITECTURE.md#31-tool-registry-pattern))
- `src-tauri/` — Tauri shell (Rust) and cargo workspace root
- `src-tauri/multitool-core/` — pure-logic rlib, testable without spinning up Tauri ([DECISIONS.md](docs/DECISIONS.md) → "Workspace split")

## License

None — personal learning project, not for distribution (see [CLAUDE.md](CLAUDE.md) "What NOT to do").
