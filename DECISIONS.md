# Decisions

Running log of noteworthy choices, caveats, and non-obvious recipes. Newest at the top. Each entry: what changed, **why**, and any impact on the codebase or workflow. For product scope see [ASSIGNMENT.md](ASSIGNMENT.md); for the architecture overview see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## 2026-05-22 — Tauri plugin baseline: `dialog` + `opener`, wrapped behind `src/lib/system.ts`

**Why.** PDF→Images C8 needs an OS file picker and an "Open output folder" affordance. A web-style `<input type="file">` is a dead-end in Tauri — the webview hides the OS path for security, so the Rust side never sees the picked file. That leaves `@tauri-apps/plugin-dialog` as effectively the only sane choice for the picker, and `@tauri-apps/plugin-opener` for `revealItemInDir` (the modern Tauri 2 replacement for `plugin-shell`-based reveals). Both are general-purpose infrastructure every future tool will reuse, so swallowing the new-dep cost once here keeps later commits smaller.

**Effect.** Added `tauri-plugin-dialog` and `tauri-plugin-opener` (Rust + JS halves), registered both in `src-tauri/src/lib.rs::run`, and granted `dialog:allow-open` + `opener:allow-reveal-item-in-dir` in `src-tauri/capabilities/default.json` — narrowest permissions that satisfy the two call sites. **All plugin calls go through `src/lib/system.ts`** (`pickPdfFile` + `revealInFolder`) so components remain presentational and Playwright keeps one mock seam at `src/lib/`. Future tools should extend `src/lib/system.ts` rather than importing `@tauri-apps/plugin-*` directly. **Choosing `plugin-opener` over `plugin-shell`:** opener has first-class `revealItemInDir`; shell's `open` doesn't reveal-in-folder cleanly and is being deprecated for this use case in Tauri 2.

---

## 2026-05-21 — Pdfium is a process-wide singleton

**Why.** pdfium-render guards its native bindings behind a global `OnceCell`: `Pdfium::bind_to_library` returns `PdfiumLibraryBindingsAlreadyInitialized` on any second call, and `Pdfium::new` `assert!`s the same invariant. C3's first attempt called `Pdfium::new` per `convert` invocation and immediately blew up in parallel `cargo test` (the first test won, every other test saw the rebind error). Sharing one instance also avoids re-loading ~5 MB of native code per call.

**Effect.** `multitool_core::pdfium::instance() -> Result<&'static Pdfium, AppError>` is the only path to a `Pdfium`; both `tools/pdf_to_images/convert` and the smoke test go through it. Implementation uses `OnceLock<Pdfium>` for the fast path plus a `Mutex<()>` to serialize the slow path so two threads can't race into a double-bind. Future tools that need pdfium: call `pdfium::instance()` and pass it around; never call `Pdfium::new` directly. If a future tool needs a *different* pdfium configuration, that's a redesign — pdfium can only be configured once per process.

---

## 2026-05-21 — pdfium binary: dynamic-load via `build.rs` download

**Why.** PDF→Images C1 needed the native pdfium library wired up cross-OS. Static linking via `pdfium-render`'s `static` feature requires libclang + a prebuilt static pdfium per OS, which would blow up CI and saddle every contributor with a Windows toolchain story. Vendoring the binaries grows the repo by ~30 MB across three platforms and turns updates into a manual chore. Requiring contributors to install pdfium system-wide breaks our reproducibility goal for a learning project. Dynamic-load with a build-time download keeps the source tree clean, builds fast, and pins exactly one upstream version.

**Effect.** `multitool-core/build.rs` downloads the platform-matched archive from <https://github.com/bblanchon/pdfium-binaries> at the pinned `chromium/7763` tag, extracts it into `OUT_DIR`, and exports the absolute library path as `PDFIUM_LIB_PATH`. The lib reads that path via `env!` in `src/pdfium.rs::bindings`. The pin matches `pdfium-render` 0.9.1's default `pdfium_7763` feature — bump the two together. `PDFIUM_LIB_PATH` can be set in the environment to bypass the download (offline builds, CI cache layer, packaged-binary override).

**Phase-1 gap to close in C6.** The baked path points into the build machine's `target/...` tree — fine for `cargo test` and `pnpm tauri dev`, broken for `pnpm tauri build` artifacts handed to a different machine. The Tauri command in C6 owns re-resolving to a Tauri resource path before the binary ships. Adding `pdfium.{so,dll,dylib}` as a bundled resource via `tauri.conf.json` is the planned approach; revisit if it gets ugly.

---

## 2026-05-21 — AppError: add `Encrypted` variant; corrupt + zero-page reuse `ProcessingFailed`

**Why.** PDF→Images planning surfaced three failure modes worth distinguishing in the UI: password-protected PDFs, corrupt PDFs, and zero-page PDFs. Only the first is meaningfully different from the user's perspective (no retry possible without password input, which Phase 1 doesn't offer); the other two are "this file is broken" with different reasons inside. Adding a variant per failure mode would over-fit the enum to one tool.

**Effect.** Add `AppError::Encrypted` (no payload — UI shows "this PDF is password-protected; Phase 1 doesn't support password entry"). Corrupt and zero-page PDFs use `ProcessingFailed { details: String }` with the underlying reason in `details`. Non-PDF inputs use the existing `UnsupportedFormat`. **General rule:** add a typed variant only when the UI branches on it; otherwise `ProcessingFailed { details }`.

---

## 2026-05-21 — Heavy deps allowed in `multitool-core` to honor the pure-fn rule

**Why.** PDF→Images's `convert` is a pure function and benefits massively from `multitool-core`'s cross-OS test coverage. But `pdfium-render` (~5MB native binary per platform) and `image` are non-trivial deps. The alternative — keep `convert` in the Tauri shell to avoid bloating core — would break the "testable without spinning up Tauri" rule from [ARCHITECTURE §3.1](ARCHITECTURE.md#31-tool-registry-pattern) and re-expose us to the Windows test-exe launch problem (see "Workspace split" entry below) on every test run.

**Effect.** `multitool-core` is allowed heavy deps when needed for pure conversion logic. Precedent for future tools (image format conversion, audio trim, ...): if the conversion fn is pure, it lives in core regardless of dep weight. The Tauri shell stays thin — IPC glue, event emission, and helpers that genuinely need Tauri APIs (e.g. resolving Tauri's app-data dir). The shell `src-tauri/src/fs/` module is reserved for the latter; pure path logic (`unique_path` etc.) goes to `multitool-core/src/fs.rs`.

---

## 2026-05-21 — Streaming `on_page` callback in multi-output conversion fns

**Why.** Encoded output for a 100-page PDF at 300 DPI in PNG can exceed 500 MB. Collecting all pages into a `Vec<PageBytes>` holds everything in memory before the caller can write it. Streaming through a callback lets the caller write-and-discard per page.

**Effect.** Pure conversion functions that produce N outputs take a `FnMut(PageOutput) -> Result<(), AppError>` callback that fires per output unit, plus a `&CancellationToken`. They return only a `JobSummary` (counts, timings), not the data. Pattern for any future tool with a 1→N shape (image format conversion across multiple files, audio segmenting, ...). Single-output tools (Images→PDF) can keep a direct `Result<Output, AppError>` return.

---

## 2026-05-21 — Test fixtures: real PDFs checked into the repo

**Why.** PDF→Images tests need a valid multi-page PDF, an encrypted PDF, and a corrupt PDF. Two options: (a) check in small real PDFs (≤ 20 KB each, ≤ 100 KB total) or (b) generate them at test-setup time. (b) is attractive for repo cleanliness but `printpdf` (our planned PDF-creation dep) can't produce encrypted or deliberately-corrupted PDFs, so we'd need a third tool for those — net more complexity for negligible disk savings.

**Effect.** Fixtures live in `multitool-core/tests/fixtures/`. **Precedent:** small representative real-world inputs are checked in; if any single fixture exceeds 1 MB, evaluate Git LFS or generate-at-test-time before committing.

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
