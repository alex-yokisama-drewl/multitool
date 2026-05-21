# multitool

[![CI](https://github.com/alex-yokisama-drewl/multitool/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/alex-yokisama-drewl/multitool/actions/workflows/ci.yml)

Cross-platform offline desktop multitool — image / PDF / etc. conversions that normally need a web app, run natively without the round trip. Tauri 2 + React + TypeScript on the frontend; Rust + Tokio on the backend. **Learning project** — not aimed at public distribution; intentionally stays in the `0.x` pre-release range.

## Project docs

- [ASSIGNMENT.md](ASSIGNMENT.md) — what the app does and how to know it's done (Phase 1 scope, acceptance criteria, roadmap)
- [ARCHITECTURE.md](ARCHITECTURE.md) — tech stack and architecture patterns
- [DECISIONS.md](DECISIONS.md) — running log of noteworthy choices and caveats
- [CLAUDE.md](CLAUDE.md) — working agreement for AI-assisted contributions; loaded into every Claude Code session
- [SCAFFOLD_PLAN.md](SCAFFOLD_PLAN.md) — _temporary_; tracks scaffold work to `v0.1.0-scaffold`, deleted after
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

- `src/` — React frontend; `src/tools/` is the tool registry ([ARCHITECTURE §3.1](ARCHITECTURE.md#31-tool-registry-pattern))
- `src-tauri/` — Tauri shell (Rust) and cargo workspace root
- `src-tauri/multitool-core/` — pure-logic rlib, testable without spinning up Tauri ([DECISIONS.md](DECISIONS.md) → "Workspace split")

## License

None — personal learning project, not for distribution (see [CLAUDE.md](CLAUDE.md) "What NOT to do").
