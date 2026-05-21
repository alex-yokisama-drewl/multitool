# CLAUDE.md

This file is loaded automatically into every Claude Code session in this repo. It captures *how we work* on this project. The *what* lives in [SPEC.md](SPEC.md) — always treat the spec as the source of truth for product scope, architecture, and tech stack decisions.

## Project intent

**Multitool** is a learning project: a cross-platform offline desktop app built to high engineering standards. It is not intended for public distribution. The project intentionally stays in the `0.x` pre-release range — never propose a `1.0.0` bump or work that only makes sense for a publishable product (macOS signing, auto-updates, public marketing, etc.) without explicit discussion.

Read [SPEC.md](SPEC.md) at the start of any non-trivial task. If a request conflicts with the spec, surface the conflict rather than silently picking one — the spec changes through discussion, not drift.

## Tech stack (summary; see SPEC §4 for full rationale)

- **Shell:** Tauri 2.x
- **Backend:** Rust (Tokio for async)
- **Frontend:** React 18 + TypeScript + Vite + Tailwind CSS + shadcn/ui
- **State:** Zustand
- **Package manager:** pnpm (do not introduce npm/yarn lockfiles)

## Working rules

### Architecture
- New tools are added as self-contained modules following the registry pattern in SPEC §5.1. Do not edit shared dashboard/routing code to add a tool.
- Heavy work runs on the Rust side via Tauri commands + Tokio tasks. The webview is rendering-only.
- Processing logic is written as pure functions (`fn(input, opts) -> Result<output>`) so it can be unit-tested without Tauri.

### Rust
- `cargo fmt` and `cargo clippy -- -D warnings` must pass before commit
- No `unwrap()` or `expect()` in non-test code; use typed `Result<T, AppError>` variants (see SPEC §5.4)
- `clippy::unwrap_used` is denied at the crate level
- Every long-running command takes a cancellation token

### TypeScript
- `strict: true` in `tsconfig.json`; no `any` without an inline justification comment
- ESLint + Prettier must pass before commit
- Components are presentational; IPC calls go through wrappers in `src/lib/`

### Testing
- Add or update tests with every behavioral change. Coverage target on Rust processing modules is ≥80% (see SPEC §8.1).
- New tool? Ship it with: Rust unit tests on the conversion function + Vitest component smoke test + Playwright happy-path e2e.
- Run the relevant test suite locally before committing. Do not push red.

### Scope discipline
- Don't add features, refactors, or abstractions beyond the task at hand. If the spec doesn't call for it, ask before building it.
- Don't introduce new dependencies without a clear reason in the PR description.

## Git & PR workflow

- **Default branch:** `master`
- **Branch model:** trunk-based; short-lived feature branches off `master`, PRs back into `master`
- **Commits:** Conventional Commits — `feat:`, `fix:`, `refactor:`, `test:`, `chore:`, `docs:`, `ci:`, `build:`
- **One logical change per PR.** Avoid mixing refactor + feature in a single PR.
- **Never force-push** to `master`. Force-pushing a feature branch is fine if it's only your work.
- **Versioning:** SemVer, but capped at `0.x` (see project intent). Tag releases on `master` as `v0.x.y`.

### Per-PR checklist (run before opening)
1. `cargo fmt && cargo clippy -- -D warnings && cargo test`
2. `pnpm lint && pnpm test && pnpm typecheck`
3. PR description states: what changed, why, how it was tested
4. CI green before requesting review / self-merging

### Using `gh` for the GitHub loop
- Open PR: `gh pr create --base master`
- Watch CI: `gh run watch`
- Read failed logs only: `gh run view --log-failed`
- Read review comments: `gh pr view <n> --json reviews,comments`

## Commands cheat sheet

> _Fill in after the 0.1.0 scaffold lands. Placeholders:_
>
> - Dev: `pnpm tauri dev`
> - Build: `pnpm tauri build`
> - Frontend unit tests: `pnpm test`
> - Frontend lint: `pnpm lint`
> - Frontend type-check: `pnpm typecheck`
> - Rust tests: `cargo test` (run from `src-tauri/`)
> - Rust lint: `cargo clippy -- -D warnings` (run from `src-tauri/`)
> - E2E: `pnpm test:e2e`

## What NOT to do

- Don't bump to `1.0.0` or propose a public release pipeline.
- Don't add macOS signing/notarization work. macOS is a CI build target only.
- Don't introduce telemetry, analytics, or any runtime network calls.
- Don't write to disk silently. The output-naming and duplicate-handling rules in SPEC §5.3 are not optional.
- Don't edit shared files to add a tool — use the registry.
- Don't skip pre-commit checks with `--no-verify`.

## When in doubt

Surface the question rather than guessing. This is a learning project and decisions are deliberate; an extra round of discussion is cheaper than undoing drift later.
