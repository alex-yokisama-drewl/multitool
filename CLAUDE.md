# CLAUDE.md

This file is loaded automatically into every Claude Code session in this repo. It captures **how we work** on this project. The deeper "what" / "how it's built" / "decision history" live in separate docs (see [Project docs](#project-docs) below).

## At a glance

**Multitool** is a cross-platform offline desktop app — image / PDF / etc. conversions, native, no web round-trip. Tauri 2.x shell, Rust + Tokio backend, React 18 + TypeScript + Vite + Tailwind + shadcn/ui frontend, Zustand for state, pnpm 9 as the package manager (no npm/yarn lockfiles). **Learning project** — not for public distribution, intentionally capped at the `0.x` range. Never propose a `1.0.0` bump or publish-oriented work (macOS signing, auto-updates, public marketing) without explicit discussion.

## Project docs

- [ASSIGNMENT.md](ASSIGNMENT.md) — product brief: what to build, why, Phase 1 acceptance, roadmap. Read at the start of any non-trivial task.
- [ARCHITECTURE.md](ARCHITECTURE.md) — tech stack, patterns (tool registry, process model, IPC, error handling), testing approach, file conventions. Read alongside ASSIGNMENT for non-trivial tasks.
- [DECISIONS.md](DECISIONS.md) — running log of noteworthy choices, caveats, recipes. Check when something looks weird; it's probably explained there.
- [CHANGELOG.md](CHANGELOG.md) — per-release notes (generated from conventional commits via `git-cliff`).

If a request conflicts with ASSIGNMENT or ARCHITECTURE, surface the conflict rather than silently picking one — those docs change through discussion, not drift.

## Working rules

### Architecture
- New tools are added as self-contained modules following the registry pattern in [ARCHITECTURE §3.1](ARCHITECTURE.md#31-tool-registry-pattern). Do not edit shared dashboard/routing code to add a tool.
- Heavy work runs on the Rust side via Tauri commands + Tokio tasks. The webview is rendering-only.
- Processing logic is written as pure functions (`fn(input, opts) -> Result<output>`) so it can be unit-tested without Tauri (lives in `multitool-core`).

### Rust
- `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings` must pass before commit
- No `unwrap()` or `expect()` in non-test code; use typed `Result<T, AppError>` variants (see [ARCHITECTURE §3.4](ARCHITECTURE.md#34-error-handling))
- `clippy::unwrap_used` is denied at the crate level
- Every long-running command takes a cancellation token

### TypeScript
- `strict: true` in `tsconfig.json`; no `any` without an inline justification comment
- ESLint + Prettier must pass before commit
- Components are presentational; IPC calls go through wrappers in `src/lib/`

### Testing
- Add or update tests with every behavioral change. Coverage target on Rust processing modules is ≥80% (see [ARCHITECTURE §4](ARCHITECTURE.md#4-testing-strategy)).
- New tool? Ship it with: Rust unit tests on the conversion function + Vitest component smoke test + Playwright happy-path e2e.
- Run the relevant test suite locally before committing. Do not push red.

### Scope discipline
- Don't add features, refactors, or abstractions beyond the task at hand. If ASSIGNMENT/ARCHITECTURE doesn't call for it, ask before building it.
- Don't introduce new dependencies without a clear reason in the PR description.

## Git & PR workflow

- **Default branch:** `master`
- **Branch model:** trunk-based; short-lived feature branches off `master`, PRs back into `master`
- **Commits:** Conventional Commits — `feat:`, `fix:`, `refactor:`, `test:`, `chore:`, `docs:`, `ci:`, `build:`
- **One logical change per PR.** Avoid mixing refactor + feature in a single PR.
- **Never force-push** to `master`. Force-pushing a feature branch is fine if it's only your work.
- **Versioning:** SemVer, but capped at `0.x` (see project intent). Tag releases on `master` as `v0.x.y`.

### Per-PR checklist (run before opening)
1. From `src-tauri/`: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p multitool-core --all-targets`
   - `--workspace` so clippy covers both the shell and `multitool-core`; `-p multitool-core` for `cargo test` because the Tauri shell's test exe fails to launch on the Windows CI runner (see [DECISIONS.md](DECISIONS.md) → "Workspace split").
2. `pnpm lint && pnpm test && pnpm typecheck`
3. From the repo root: `pnpm tauri build --no-bundle` — only step that compiles the Tauri shell with `--release`; catches build-only regressions CI would otherwise be the first to see
4. PR description states: what changed, why, how it was tested
5. CI green before requesting review / self-merging

### Using `gh` for the GitHub loop
- Open PR: `gh pr create --base master`
- Watch CI: `gh run watch`
- Read failed logs only: `gh run view --log-failed`
- Read review comments: `gh pr view <n> --json reviews,comments`

## Commands cheat sheet

All `pnpm` commands run from the repo root; all `cargo` commands run from `src-tauri/`.

- **Dev / build:** `pnpm tauri dev` · `pnpm tauri build --no-bundle` (compile-only) · `pnpm tauri build` (full bundle)
- **Frontend gates:** `pnpm lint` · `pnpm typecheck` · `pnpm format:check` · `pnpm test` · `pnpm test:coverage` · `pnpm test:e2e`
- **Rust gates:** `cargo fmt --all --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test -p multitool-core --all-targets` · `cargo llvm-cov --summary-only -p multitool-core`
- **Hooks:** `pnpm exec lefthook run pre-commit` · `pnpm exec lefthook run pre-push`

`-p multitool-core` on `cargo test`/`cargo llvm-cov` keeps the Tauri-shell test exe out of the run; it can't launch on the Windows CI runner (see [DECISIONS.md](DECISIONS.md) → "Workspace split").

## What NOT to do

- Don't bump to `1.0.0` or propose a public release pipeline.
- Don't add macOS signing/notarization work. macOS is a CI build target only.
- Don't introduce telemetry, analytics, or any runtime network calls.
- Don't write to disk silently. The output-naming and duplicate-handling rules in [ARCHITECTURE §3.3](ARCHITECTURE.md#33-file-io-conventions) are not optional.
- Don't edit shared files to add a tool — use the registry.
- Don't skip pre-commit checks with `--no-verify`.

## When in doubt

Surface the question rather than guessing. This is a learning project and decisions are deliberate; an extra round of discussion is cheaper than undoing drift later.
