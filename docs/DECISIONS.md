# Decisions

Choices, caveats, and recipes that affect future work — patterns we must keep following, or non-obvious workarounds we want to make sure don't get reverted. Newest first. Architecture overview: [ARCHITECTURE.md](ARCHITECTURE.md). Plans and ideas: [plans/BACKLOG.md](plans/BACKLOG.md).

**Keep entries short.** Only spend more than a few lines when there's a real risk someone may want to revert the decision and re-do the work. For choices between equally valid options, or where the alternative obviously breaks the app, a sentence or two is enough.

---

## pdfium: bundle native binary as a Tauri resource

PDF→Images needs `pdfium.{dll,so,dylib}` available at runtime; baking `env!("PDFIUM_LIB_PATH")` into the binary leaks the CI runner's path and breaks on end-user machines.

- `multitool_core::pdfium::init(path)` accepts a runtime path; `instance()` prefers it and falls back to the compile-time env var so dev / `cargo test` keep working.
- [`src-tauri/build.rs`](../src-tauri/build.rs) downloads the bblanchon binary and copies it into `src-tauri/resources/pdfium/` (gitignored); [`tauri.conf.json`](../src-tauri/tauri.conf.json) bundles it as a resource; the shell's `.setup` hook calls `pdfium::init` before any command runs.
- Both `multitool-core/build.rs` and `src-tauri/build.rs` download the same archive. Cross-crate metadata via `links =` is more code than the ~30-line copy. **If the pdfium pin moves, update both files together.**

---

## WebP output is lossless only (no `webp_quality` option)

`image` 0.25's WebP encoder is lossless-only. `TargetFormat::Webp` in image-format-converter emits lossless WebP unconditionally — no `webp_quality` field in `Opts`. A lossy lane would mean the `webp` crate (libwebp C bindings + native build dep on every CI runner) or `image-webp` (lossless-only encoder at last check). **Don't add a silent quality field that gets ignored** — if lossy WebP is needed, weigh the libwebp dep in a new entry first.

---

## Staging-area reorder: `@dnd-kit/sortable`

Picked `@dnd-kit/sortable` + `@dnd-kit/core` for the Images → PDF staging grid. The brief mandates mouse AND keyboard reorder; `@dnd-kit` has keyboard reorder, screen-reader announcements, and touch/pointer sensors out of the box. Rejected: HTML5 DnD (no keyboard a11y), `react-beautiful-dnd` (archived 2023), `react-dnd` (heavier API, same HTML5-backend a11y limits). Two packages added against the "no deps without reason" rule — justified by the a11y requirement.

---

## Asset protocol scope: dynamic per-pick, not static glob

Webview thumbnail previews (`convertFileSrc(path)`) require the resolved path to be in Tauri's asset-protocol scope. We grant nothing by default and allow each picked path at runtime:

- [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json): `app.security.assetProtocol = { enable: true, scope: [] }`.
- [src-tauri/src/asset_scope.rs](../src-tauri/src/asset_scope.rs): `allow_image_preview(paths)` command calls `app.asset_protocol_scope().allow_file(p)` per path. **Re-validates extensions server-side** — the OS picker's filter is advisory; a direct IPC call could bypass it.
- `tauri` crate needs the `protocol-asset` feature.

A static glob (e.g. `**/*.png`) would expose every matching file on disk, not just picked ones. **Future-tool pattern:** any tool wanting webview-side resource access from user-picked paths should add an `allow_*_preview` command on the same model. Don't extend the static scope.

---

## Tauri plugin baseline: `dialog` + `opener`, wrapped behind `src/lib/system.ts`

`tauri-plugin-dialog` (file picker — `<input type="file">` is a dead-end in Tauri; the webview hides the OS path) and `tauri-plugin-opener` (`revealItemInDir`) are registered in [src-tauri/src/lib.rs](../src-tauri/src/lib.rs) with capabilities granted in [src-tauri/capabilities/default.json](../src-tauri/capabilities/default.json). **All plugin calls go through [src/lib/system.ts](../src/lib/system.ts)** — components stay presentational and Playwright keeps one mock seam. Future tools should extend `system.ts` rather than importing `@tauri-apps/plugin-*` directly. New plugins need their own DECISIONS entry and the narrowest-possible capability grant.

---

## Pdfium is a process-wide singleton

`pdfium-render` guards its native bindings behind a global `OnceCell` — any second `Pdfium::new` blows up (`PdfiumLibraryBindingsAlreadyInitialized`), which kills parallel `cargo test` runs. Use `multitool_core::pdfium::instance() -> Result<&'static Pdfium, AppError>`; never call `Pdfium::new` directly. If a future tool needs a different pdfium configuration, that's a redesign — pdfium can only be configured once per process.

---

## pdfium binary: dynamic-load via `build.rs` download

[multitool-core/build.rs](../src-tauri/multitool-core/build.rs) downloads pdfium from <https://github.com/bblanchon/pdfium-binaries> at the pinned `chromium/7763` tag and exports `PDFIUM_LIB_PATH`. The pin must move with `pdfium-render`'s default feature (currently `pdfium_7763`) — bump the two together. `PDFIUM_LIB_PATH` can be set in the environment to bypass the download (offline builds, CI cache, packaged-binary override). Static linking was rejected (needs libclang + prebuilt static pdfium per OS); vendoring was rejected (~30 MB across three platforms).

---

## `AppError`: typed variant only when the UI branches on it

Variants are limited to ones the UI distinguishes meaningfully: `FileNotFound`, `PermissionDenied`, `UnsupportedFormat`, `ProcessingFailed { details }`, `Encrypted`, `Cancelled`. Anything else uses `ProcessingFailed { details }` with the underlying reason in `details`. Adding a variant per failure mode over-fits the enum to one tool.

---

## Heavy deps allowed in `multitool-core`

Pure conversion functions live in `multitool-core` regardless of dep weight (pdfium ~5 MB, `image`, etc.). Keeping them in the Tauri shell instead would break the "testable without Tauri" rule from [ARCHITECTURE §3.1](ARCHITECTURE.md#31-tool-registry-pattern) and re-expose the Windows test-exe launch problem (see "Workspace split" below). The shell stays thin — IPC glue, event emission, and helpers that genuinely need Tauri APIs (e.g. resolving Tauri's app-data dir).

---

## Streaming `on_page` callback in multi-output conversion fns

Pure conversion fns that produce N outputs take a `FnMut(PageOutput) -> Result<(), AppError>` callback plus a `&CancellationToken`, and return only a `JobSummary`. Encoded output for large jobs (a 100-page PDF at 300 DPI in PNG can exceed 500 MB) shouldn't be held in memory. Apply to any 1→N tool (image format conversion across many files, audio segmenting, …); single-output tools return `Result<Output, AppError>` directly.

---

## Test fixtures: real files checked into the repo

Small representative real-world inputs (≤ 20 KB each, ≤ 100 KB total per tool) live in `multitool-core/tests/fixtures/`. Generating fixtures at test time was rejected because not all required artifacts (encrypted PDFs, deliberately-corrupted files) can be produced by our existing deps. If any single fixture exceeds 1 MB, evaluate Git LFS or generate-at-test-time first.

---

## No `default-members` on the cargo workspace

`src-tauri/Cargo.toml` declares no `default-members`. `tauri build` runs `cargo build --bins --features tauri/custom-protocol`; with `default-members = ["multitool-core"]` (a non-Tauri crate) the feature flag misroutes and the build dies on every OS. CI and lefthook pass `cargo test -p multitool-core --all-targets` explicitly because the shell's test exe can't launch on Windows (see "Workspace split" below).

---

## Workspace split: `multitool-core` rlib

`multitool-core` exists because the Tauri shell's test exe fails to launch on the Windows CI runner (`STATUS_ENTRYPOINT_NOT_FOUND` / `0xC0000139`, traced to `ProcessPrng` in `bcryptprimitives.dll` imported transitively via `tauri → getrandom 0.3.4`). Consequence: **the shell has no test lane** — everything worth testing must live in `multitool-core`. CI and lefthook both run `cargo test -p multitool-core --all-targets`.

---

## `pnpm.packageExtensions` for the vitest vite peer

`package.json` injects `@types/node` into vitest's peer set so TypeScript sees a single `Plugin` type and can accept `react()` / `tailwindcss()` in the vite config. If a future devDep reintroduces a no-`@types/node` vite peer, expect the same diagnostic (two `Plugin` types) and the same fix.
