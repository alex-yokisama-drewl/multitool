# Scaffold Plan — to `v0.1.0-scaffold`

Plan for taking the repo from "spec + CLAUDE.md only" to a buildable scaffold with all toolchains, lint/test/CI wired, and registry stubs in place. **No feature code** — the first tool ships in a follow-up PR.

Phases run roughly in order; within a phase, sub-tasks can be parallelized. Each phase ends with a checkpoint commit so we can bisect later.

---

## Phase A — Local prerequisites — DONE (2026-05-21)

All toolchains and Linux system libs verified/installed on the dev box: Rust
1.95 via rustup, pnpm 9 via corepack on Node 20, plus Tauri's webkit2gtk-4.1 /
ayatana-appindicator3 / librsvg2 / libsoup-3.0 dev libs. Per-machine setup,
nothing committed.

**Dev-environment note:** VS Code installed via snap leaks confinement env
(`/snap/core20/...` libpthread) into its integrated terminal, which makes
`pnpm tauri dev` fail at runtime with a `GLIBC_PRIVATE` symbol lookup error.
Launch the dev shell from a non-VS-Code terminal, or replace the snap install
with the `.deb` build.

---

## Phase B — Project initialization — DONE in `chore: scaffold tauri 2 + react + ts template`

Scaffolded via `create-tauri-app` (React + TypeScript + Vite, pnpm template),
trimmed the boilerplate UI and `greet` command down to an empty shell, pinned
React 18 per SPEC §4 (template defaulted to 19), dropped `tauri-plugin-opener`
(unused after removing `greet`), and rewrote the Tauri entry point so the
crate-level `deny(clippy::expect_used)` planned for Phase D will not need to
special-case it.

---

## Phase C — Frontend configuration

1. Install and configure **Tailwind CSS** (PostCSS, base/components/utilities, content globs).
2. Initialize **shadcn/ui** (`pnpm dlx shadcn@latest init`). Do not pre-install components — pull them in as tools need them.
3. Add **Zustand** and **React Router** (configure hash router per SPEC §4).
4. Set `tsconfig.json` → `strict: true`, `noUncheckedIndexedAccess: true`, path alias `@/*` → `src/*`.
5. ESLint + Prettier (flat config), with the React + TS plugins. Lint rule: no `any` without an inline justification comment.
6. Build out the directory skeleton from SPEC §9:
   ```
   src/{app,components,tools,lib}/
   src/tools/registry.ts   # exports an empty Tool[] for now
   ```
7. Minimal app shell: a placeholder dashboard route that reads from the (empty) registry and shows "no tools yet."
8. **Checkpoint commit:** `chore: configure frontend toolchain (tailwind, shadcn, zustand, router)`

---

## Phase D — Backend configuration

1. In `src-tauri/Cargo.toml`, add baseline deps: `tokio` (with `rt-multi-thread`, `macros`, `sync`), `serde`, `serde_json`, `thiserror`, `tracing`, `tracing-subscriber`.
   - Media deps (`image`, `pdfium-render`, `printpdf`) land with the tool that needs them, not now.
2. Crate-level `#![deny(clippy::unwrap_used, clippy::expect_used)]` in `main.rs` / `lib.rs`.
3. Define `AppError` enum per SPEC §5.4 with `thiserror`; wire serde for IPC serialization.
4. Stub `src-tauri/src/{tools,ipc,fs}/` per SPEC §9, each with `mod.rs`. `tools/mod.rs` exports an empty command list / registry.
5. Stub the cancellation-token plumbing (job ID → `CancellationToken` map) so the first tool can plug into it.
6. Confirm `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test` (zero tests) all pass.
7. **Checkpoint commit:** `chore: configure rust backend (error type, tokio, registry stubs)`

---

## Phase E — Quality tooling

1. **`lefthook`** for pre-commit: `cargo fmt`, `cargo clippy -- -D warnings` (changed files), `pnpm lint`, `pnpm typecheck`. Pre-push: full test run.
2. `.editorconfig`, `.gitattributes` (LF line endings, mark `Cargo.lock` / `pnpm-lock.yaml` as merge=union or similar).
3. `.gitignore` covering `target/`, `dist/`, `node_modules/`, OS junk, IDE files.
4. **`git-cliff`** config for changelog generation from Conventional Commits (SPEC §8.2).
5. **Checkpoint commit:** `chore: add lefthook hooks and changelog config`

---

## Phase F — Test harnesses (no real tests yet, just plumbing)

1. **Vitest** + **@testing-library/react** + jsdom env. One trivial passing test to prove wiring.
2. **Playwright** with a Tauri-aware launcher (or `webdriver`-based driver). One placeholder spec that opens the app shell and asserts the dashboard title — marked `.skip` until the first tool exists.
3. Rust: a `tests/` integration dir + a `#[cfg(test)]` smoke test in `tools/mod.rs`. Confirm `cargo tarpaulin` or `cargo llvm-cov` runs (we need coverage measurement for the ≥80% target in SPEC §8.1).
4. **Checkpoint commit:** `chore: add test harnesses (vitest, playwright, coverage)`

---

## Phase G — CI

GitHub Actions workflows under `.github/workflows/`:

1. `ci.yml` — runs on PR + push to `master`:
   - matrix: `ubuntu-latest`, `windows-latest`, `macos-latest`
   - steps: install toolchains (cache `~/.cargo`, `~/.pnpm-store`, `target/`), `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `pnpm install --frozen-lockfile`, `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm tauri build` (verify only; don't upload).
2. `release.yml` — on `v*` tag push: build per-platform artifacts, attach to GitHub Release. **No** macOS signing/notarization (CLAUDE.md rule).
3. Branch protection for `master`: require CI green + linear history.
4. **Checkpoint commit:** `ci: add cross-platform pipeline and release workflow`

---

## Phase H — Repo polish

1. `README.md`: one-screen intro pointing at SPEC.md and CLAUDE.md; quickstart (`pnpm install`, `pnpm tauri dev`).
2. `CHANGELOG.md`: empty header generated by `git-cliff` template.
3. Fill in the **commands cheat sheet** placeholder in CLAUDE.md with the real commands now that scripts exist.
4. `LICENSE`: skip — learning project, not for distribution (CLAUDE.md rule). Revisit only if intent changes.
5. **Checkpoint commit:** `docs: scaffold readme, changelog, commands cheat sheet`

---

## Phase I — Tag

1. Open a single PR (or merge the checkpoint commits) into `master`.
2. After merge: tag `v0.1.0-scaffold` on `master`. This is the baseline for feature work; the actual `v0.1.0` waits until both Phase 1 tools ship.

---

## Definition of done

The scaffold is complete when **all** of the following are true on a clean clone:

- `pnpm install && pnpm tauri dev` opens the dashboard shell on Linux and Windows.
- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` pass with zero warnings.
- `pnpm lint`, `pnpm typecheck`, `pnpm test` pass.
- Lefthook pre-commit blocks a deliberately-bad commit (manual sanity check).
- CI passes on all three OSes for a no-op PR.
- Adding a new tool requires only: one folder under `src/tools/`, one folder under `src-tauri/src/tools/`, one import line in each registry. No edits to shared shell/routing code (SPEC §5.1).

---

## Explicit non-goals for the scaffold PR

To keep this PR reviewable and avoid scope creep:

- No feature code (no PDF↔image logic).
- No drag-and-drop input (Phase 2 per SPEC §10).
- No telemetry, no auto-updater, no signing pipeline.
- No production-grade theming — default shadcn theme is fine.

## Resolved tooling choices

- **E2E driver**: Playwright against `pnpm dev` (the Vite server), with Tauri IPC mocked at the `src/lib/` wrapper boundary. SPEC §8.1 asks for Playwright by name, and the official Tauri WebDriver path (`tauri-driver`) is WebdriverIO-only. This gives us happy-path UI coverage now; if we later need real desktop-shell coverage we can add WebdriverIO + `tauri-driver` as a second e2e lane without ripping out Playwright.
- **Rust coverage**: `cargo-llvm-cov`. Works on all three CI OSes; `cargo-tarpaulin` is effectively Linux-only and we need the coverage gate to run on the same matrix as the rest of CI.

Both are reversible later — flagged here so the scaffold PR description can call them out for review.
