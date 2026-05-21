# Decisions

Running log of noteworthy choices, caveats, and non-obvious recipes. Newest at the top. Each entry: what changed, **why**, and any impact on the codebase or workflow. For product scope see [ASSIGNMENT.md](ASSIGNMENT.md); for the architecture overview see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## 2026-05-21 — Branch protection on `master` is classic, admin-bypass

**Why.** Phase G's Definition-of-done requires the three CI contexts to pass on a PR before merge, and linear history must be preserved. Solo learning project, so external review and admin enforcement are not warranted.

**Effect.** Classic branch protection on `master` requires `linux` / `macos` / `windows` contexts and `required_linear_history`; force-push and branch deletion are blocked. `enforce_admins` stays `false` so the owner can bypass when needed. Rulesets are empty — protection is classic-only.

---

## 2026-05-21 — GitHub Actions bumped to v6 (Node 24 runtime)

**Why.** `actions/checkout@v4`, `actions/setup-node@v4`, and `pnpm/action-setup@v4` run on Node 20, which GitHub deprecated for Actions runners (force-upgrade to Node 24 on 2026-06-02; Node 20 removed from runners 2026-09-16). Every CI run was emitting a deprecation warning.

**Effect.** Bumped all three to `@v6` in both `ci.yml` and `release.yml`. No input-shape changes (`version: 9`, `node-version: 20`, `cache: pnpm` still valid). If a future regression forces a rollback, `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true` keeps v4 working on Node 24.

---

## 2026-05-21 — No `default-members` on the cargo workspace

**Why.** `tauri build` runs `cargo build --bins --features tauri/custom-protocol`, and cargo applies the feature flag to the selected package. The post-split workspace originally declared `default-members = ["multitool-core"]`, which redirected the feature flag to a crate with no tauri dep. Build died on every OS.

**Effect.** `src-tauri/Cargo.toml` declares no `default-members`. As a root-crate workspace, cargo defaults to the shell when run from `src-tauri/`. CI and lefthook pass `cargo test -p multitool-core --all-targets` explicitly to keep the shell's test exe out of the run (it can't launch on Windows — see the next entry).

---

## 2026-05-21 — Workspace split: `multitool-core` rlib

**Why.** Initial CI runs failed on Windows with `STATUS_ENTRYPOINT_NOT_FOUND` (0xC0000139) when launching the Tauri shell's test exe. `dumpbin /IMPORTS` traced this to `ProcessPrng` in `bcryptprimitives.dll`, imported transitively via `tauri → getrandom 0.3.4`. The symbol fails to resolve at exe-launch time on the Windows Server 2025 runner image, even though Microsoft documents it as available there. Two intermediate hypotheses (dropping `cdylib`/`staticlib` from `crate-type`; verifying the OS image via `dumpbin /EXPORTS`) ruled out the obvious culprits.

**Effect.** Created `src-tauri/multitool-core/` as a pure-logic rlib (no tauri dep) and moved `AppError`, `JobId`, and `JobRegistry` (plus their tests + the integration smoke from `src-tauri/tests/`) into it. The shell at `src-tauri/` depends on `multitool-core` and keeps the bits that genuinely need tauri (`cancel_job`, `register_commands`, `run()`). Aligns with [ARCHITECTURE.md §3.1](ARCHITECTURE.md#31-tool-registry-pattern) — pure functions, testable without spinning up Tauri.

---

## 2026-05-21 — pnpm `packageExtensions` for the vitest vite peer

**Why.** Vitest's transitive vite peer-resolution created a second vite graph (no `@types/node` peer) while the project's own vite carried `@types/node`. TypeScript saw two `Plugin` types and refused to assign `react()` / `tailwindcss()` to the config's `plugins` array.

**Effect.** Added `pnpm.packageExtensions` in `package.json` to inject `@types/node` into vitest's peer set. Collapses the graph to a single vite resolution. If a future devDep reintroduces a no-`@types/node` vite peer, expect the same diagnostic and the same fix.

---

## 2026-05-21 — ESLint 9 (not 10)

**Why.** `eslint-plugin-react@7.37` peer-deps cap at ESLint 9; bumping to 10 throws at lint time.

**Effect.** ESLint pinned to `^9` in `package.json`. Revisit when the plugin catches up — nothing automated currently watches this; periodic manual check on `eslint-plugin-react` releases.

---

## 2026-05-21 — Tailwind v4 via `@tailwindcss/vite`

**Why.** Tailwind v4's modern setup uses the Vite plugin directly — no PostCSS config, no `tailwind.config.js`.

**Effect.** Single `@import "tailwindcss"` in the CSS entry. Side effect: shadcn's generated CSS imports `shadcn/tailwind.css`, so `shadcn` is a runtime dependency, not just a CLI tool.

---

## 2026-05-21 — Playwright (not WebdriverIO/tauri-driver) for e2e

**Why.** ARCHITECTURE §4 specifies Playwright. The official Tauri WebDriver path (`tauri-driver`) is WebdriverIO-only, but happy-path UI coverage is fine against the Vite dev server with the Tauri layer mocked at the `src/lib/` boundary.

**Effect.** Playwright drives `pnpm dev`. Tauri IPC wrappers in `src/lib/` are the mock surface. If we later need real desktop-shell coverage we can add WebdriverIO + `tauri-driver` as a second e2e lane without ripping out Playwright.

---

## 2026-05-21 — `cargo-llvm-cov` for Rust coverage

**Why.** Works on all three CI OSes; `cargo-tarpaulin` is effectively Linux-only and we need the coverage gate to run on the same matrix as the rest of CI.

**Effect.** `cargo llvm-cov --summary-only -p multitool-core` is the canonical command. Not added as a project dep — install once on the machine that runs coverage (`cargo install cargo-llvm-cov`).

---

## 2026-05-21 — `lefthook` for git hooks (not husky + lint-staged)

**Why.** Single-binary cross-platform runner with glob-gated job filters. Cleaner than husky + lint-staged for our use case.

**Effect.** `lefthook` added as a devDep (npm wrapper that ships the binary); `prepare: lefthook install` wires hooks on every `pnpm install`. Pre-commit glob-gates fmt/lint per file type; pre-push runs full test suites.

---

## 2026-05-21 — VS Code snap leaks libpthread into terminal

**Why.** VS Code installed via snap leaks confinement env (`/snap/core20/...` libpthread) into its integrated terminal, which makes `pnpm tauri dev` fail at runtime with a `GLIBC_PRIVATE` symbol lookup error.

**Effect.** Launch the dev shell from a non-VS-Code terminal, or replace the snap install with the `.deb` build. Per-machine workaround, not committable.
