# Scaffold Plan — to `v0.1.0-scaffold`

Plan for taking the repo from "spec + CLAUDE.md only" to a buildable scaffold with all toolchains, lint/test/CI wired, and registry stubs in place. **No feature code** — the first tool ships in a follow-up PR.

Phases run roughly in order; within a phase, sub-tasks can be parallelized. Each phase ends with a checkpoint commit so we can bisect later.

## Working through this plan

When a Claude session executes a phase:

- **Mark the phase complete in this doc once it lands.** Replace the detailed sub-task list with a short prose summary in the same style as the already-done phases below (one paragraph; reference the checkpoint commit subject).
- **Prune anything no longer relevant.** Resolved questions, completed sub-tasks, transient dev-env notes that no longer apply — delete them. The plan should describe what's left to do, not what was done.
- **Pause and ask for confirmation before starting the next phase.** Don't chain phases together unprompted.

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

## Phase C — Frontend configuration — DONE in `chore: configure frontend toolchain (tailwind, shadcn, zustand, router)`

Tailwind v4 wired via `@tailwindcss/vite` (no PostCSS, no `tailwind.config.js`
— modern default; diverges from the v3 wording the plan originally had).
shadcn/ui initialized with the Nova preset (radix base, neutral color); no
components pre-installed. Zustand and `react-router-dom` added; the router
uses hash mode per SPEC §4. `tsconfig.json` gained `noUncheckedIndexedAccess`
and the `@/*` path alias (`baseUrl` dropped — deprecated in TS 6). ESLint 9
flat config wires typescript-eslint type-checked rules plus the React /
Hooks / Refresh plugins, with `@typescript-eslint/no-explicit-any` as an
error (suppressions require an inline justification per CLAUDE.md). Prettier
runs alongside via `eslint-config-prettier`; markdown and `src-tauri/` are
prettier-ignored. The directory skeleton from SPEC §9
(`src/{app,components,tools,lib}/`) exists; `src/tools/registry.ts` exports
an empty `Tool[]`; a placeholder `Dashboard` route reads the registry and
renders a "no tools yet" empty state.

**Heads-up for Phase G:** ESLint had to be pinned to `^9` —
`eslint-plugin-react@7.37` does not yet support ESLint 10. Revisit when the
plugin updates so CI doesn't drift onto a deprecated major.

---

## Phase D — Backend configuration — DONE in `chore: configure rust backend (error type, tokio, registry stubs)`

Baseline Rust deps added (`tokio` rt-multi-thread/macros/sync, `tokio-util`
for `CancellationToken`, `thiserror`, `tracing` + `tracing-subscriber`); media
crates wait for the first tool. `AppError` (SPEC §5.4 variants) lives in
`error.rs` and serializes as `{ kind, message }` so the webview can branch on
`kind`. Module skeleton from SPEC §9 in place: `tools/`, `ipc/`, `fs/`.
`tools::register_commands` is the single edit point for adding a tool —
`lib.rs` calls it once via `Builder`, so no shared-shell edits are needed
when tools are added. `ipc::JobRegistry` maps `JobId → CancellationToken`
behind a `Mutex` with poison-recovery (`unwrap_or_else(|p| p.into_inner())`
keeps `clippy::unwrap_used` honest); `cancel_job` is the first registered
command and gives `generate_handler!` a non-empty list. `lib.rs` wires
`tracing_subscriber::fmt().with_env_filter(...).try_init()` and manages the
registry. Crate-level lints are gated with `cfg_attr(not(test), ...)` so unit
tests in `error.rs` / `ipc/mod.rs` can use `.unwrap()` freely.

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

- **Tailwind v4** (settled in Phase C). Uses `@tailwindcss/vite` and a single `@import "tailwindcss"` in the CSS entry — no PostCSS config, no `tailwind.config.js`. Side effect: shadcn's generated CSS imports `shadcn/tailwind.css`, so `shadcn` is a runtime dependency, not just a CLI tool.
- **shadcn/ui Nova preset** (settled in Phase C). Radix base, neutral color, Lucide icons, Geist Variable font. The default theme is fine per the SPEC scaffold non-goals; revisit if the app ever needs custom branding.
- **ESLint 9 (not 10)** (settled in Phase C). `eslint-plugin-react@7.37` peer-deps cap at ESLint 9; bumping to 10 throws at lint time. Track upstream and bump when the plugin catches up.
- **E2E driver**: Playwright against `pnpm dev` (the Vite server), with Tauri IPC mocked at the `src/lib/` wrapper boundary. SPEC §8.1 asks for Playwright by name, and the official Tauri WebDriver path (`tauri-driver`) is WebdriverIO-only. This gives us happy-path UI coverage now; if we later need real desktop-shell coverage we can add WebdriverIO + `tauri-driver` as a second e2e lane without ripping out Playwright.
- **Rust coverage**: `cargo-llvm-cov`. Works on all three CI OSes; `cargo-tarpaulin` is effectively Linux-only and we need the coverage gate to run on the same matrix as the rest of CI.

All reversible later — flagged here so the scaffold PR description can call them out for review.
