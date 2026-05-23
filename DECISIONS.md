# Decisions

Choices, caveats, and recipes that affect future work — patterns we must keep following, or non-obvious workarounds we want to make sure don't get reverted. Newest first. Product scope: [ASSIGNMENT.md](ASSIGNMENT.md). Architecture overview: [ARCHITECTURE.md](ARCHITECTURE.md).

---

## 2026-05-23 — pdfium: bundle native binary as a Tauri resource

The v0.2.0 installer shipped without `pdfium.dll` next to the binary. `multitool_core::pdfium::instance` baked an `env!("PDFIUM_LIB_PATH")` pointing into the GitHub Actions runner's `target/.../OUT_DIR/pdfium/...`, so on an end user's machine `LoadLibraryExW` returned error 126 ("module not found") and PDF→Images failed with `pdfium bind failed: LoadLibraryError(...)`. Image Format Converter and Images→PDF were unaffected (only PDF→Images uses pdfium).

Fix (landed in v0.2.1):

- `multitool-core::pdfium::init(path)` accepts a runtime path; `instance()` prefers it and falls back to the compile-time env var (so dev / `cargo test` keep working without setup).
- [`src-tauri/build.rs`](src-tauri/build.rs) downloads the bblanchon binary into `OUT_DIR` (mirroring `multitool-core/build.rs`) and copies it to `src-tauri/resources/pdfium/{libpdfium.so,libpdfium.dylib,pdfium.dll}`. `resources/pdfium/` is gitignored — it's a build artifact.
- [`tauri.conf.json`](src-tauri/tauri.conf.json) bundles `resources/pdfium/*`.
- The Tauri shell's `.setup` hook calls `pdfium::init(app.path().resolve("resources/pdfium/<file>", BaseDirectory::Resource))` before any command runs.

Two build scripts now download the same archive (cached per `OUT_DIR`); duplicating the ~30 lines is cheaper than cross-crate metadata via `links =` for a one-off pin. If the pdfium pin moves, update both `build.rs` files together. Adding a "load the bundled installer and exercise PDF→Images" e2e step would have caught this — captured as a follow-up, not actioned here.

---

## 2026-05-23 — Known caveat: MSI installer aborted with "another installation in progress" on one Windows machine

v0.2.0's `multitool_0.2.0_x64_en-US.msi` failed to install on the maintainer's Windows machine — Windows Installer reported another install was in progress and aborted. The `_x64-setup.exe` (NSIS) installer worked on the same machine. Almost certainly a stuck Windows Installer service or transient MSI lock on that host, not a bundle problem (Tauri's MSI is a stock WiX layout). Note for future testing: if this reproduces on a clean Windows runner / second machine, investigate WiX config; otherwise keep treating it as host-specific. Both `.msi` and `.exe` are still attached to releases.

---

## 2026-05-22 — Phase F generalization audit: three extracts, one deliberate skip

After image-format-converter shipped, three tools share enough boilerplate to look at. Four candidates audited; three extracted, one deliberately left inline.

**Extracted:**

- **`multitool_core::image::decode_oriented(source_ext, bytes)`** ([multitool-core/src/image.rs](src-tauri/multitool-core/src/image.rs)) — EXIF-orientation-aware decode + extension-based format fallback for magic-less formats (TGA). Both `images_to_pdf::convert` and `image_format_converter::convert` had their own copy with diverged signatures; the strictly-more-general form (with `source_ext`) wins. Errors are context-free; tools wrap with path context at the call site if they care. Companion: `image_to_app_err(err)`.
- **`crate::ipc::run_streaming_job(app, registry, job_id, run)`** ([src-tauri/src/ipc/streaming_job.rs](src-tauri/src/ipc/streaming_job.rs)) — All three `#[tauri::command]` shims duplicated ~80 lines of "register → spawn_blocking → emit progress → unregister → emit complete/error". The helper is generic over the `Progress`/`Result` payload types; each shim is now a 6-line closure that calls its `run_job`. Required normalizing `image_format_converter::run_job`'s arg order to match the other two (`inputs, opts, cancel, on_progress`).
- **`fileName(path)` + `fileStem(path)`** ([src/lib/utils.ts](src/lib/utils.ts)) — Two-line path helpers duplicated across both staging tools. Trivial extraction but utilities live in a single home so a fourth tool picks them up for free.

**Skipped — staging-grid thumbnail card** (see next entry).

**Skipped on principle** (not new audit findings):

- **Job orchestrators** (`run_job` in each tool) — 1→N pages, N→1 PDF, N→N files. Three real variations on output shape; the inner loops aren't the same pattern.
- **Quality / clamp option patterns** — DPI clamp in `pdf_to_images`, JPEG/SVG-px clamps in `image_format_converter`. `val.clamp(MIN, MAX)` already is the helper; the constants are per-tool.

The rule for a fourth tool: consume from the shared surfaces above. If the duplication that emerges doesn't match any shared surface, leave it inline for one more tool before extracting — `bias toward (a)` from [CLAUDE.md → Scope discipline](CLAUDE.md).

---

## 2026-05-22 — Staging-grid thumbnail card stays inline (not extracted)

`images_to_pdf` and `image_format_converter` both render a grid of `<li>{img, filename, × button}` cards over staged paths. Tempting to extract — but the wrappers diverge non-trivially:

- `images_to_pdf` wraps the img + filename in a `<button>` that doubles as a `useSortable` drag handle for reordering. The remove button calls `stopPropagation()` to keep `×` clicks from initiating drags.
- `image_format_converter` has no reordering at all (order is irrelevant for the conversion output).

A shared `<ImageThumbCard>` would force optional drag-handle props (or two separate variants) onto a component whose visual core is only four lines of markup. Per the [adding-a-tool.md](docs/adding-a-tool.md) extraction bar — "extract only if (a) bodies are nearly identical, (b) future tools likely want the same surface, (c) extraction doesn't force divergent tools to grow optional knobs" — (c) fails here.

The duplication is ~4 lines per consumer (the `<img>` + `<span>` content). Not zero, but the alternative — a multi-mode component or render-prop API — would be more code, not less. Reassess when a third staging tool lands; if it also doesn't need reorder, that tips (a)/(b) far enough to justify the extraction.

The two utilities that *are* shared (`imageAssetUrl` + `allowImagePreview` from `src/lib/system.ts`, and `fileName` from `src/lib/utils.ts`) already cover the load-bearing pieces.

---

## 2026-05-22 — WebP output is lossless only (no `webp_quality` option)

`image` 0.25's WebP encoder (`image::codecs::webp::WebPEncoder::new_lossless`) is lossless-only — it exposes no quality knob. The image-format-converter tool reflects this: there is no `webp_quality` field in `Opts`, and `TargetFormat::Webp` always emits lossless WebP. Decode handles both lossy and lossless WebP inputs unchanged (decoder feature parity isn't the constraint).

A lossy WebP encode lane would require switching to the `webp` crate (libwebp C bindings + a native build dep on every CI runner) or `image-webp` (pure Rust but encoder-only-lossless at last check). Neither's worth the dep + matrix cost while no user has asked for lossy WebP output.

If a future tool needs lossy WebP, the right move is a `DECISIONS.md` entry weighing the libwebp dep and adding it to `multitool-core` — not adding a silent quality field that gets ignored.

---

## 2026-05-22 — Staging-area reorder: `@dnd-kit/sortable` over native HTML5 / react-beautiful-dnd

Images → PDF stages picked images in a grid the user reorders before generating the PDF. The brief requires both mouse AND keyboard reorder, so the drag-and-drop choice has to be accessible from day one. Picked `@dnd-kit/sortable` (+ `@dnd-kit/core` peer).

**vs the browser-native HTML5 DnD API:** poor accessibility (no built-in keyboard support, screen-reader announcements need hand-rolling), inconsistent across browsers and platforms, the drag-image ghost is hard to style cleanly. Building a sortable grid on top is doable but reinvents what `@dnd-kit` solves out of the box.

**vs `react-beautiful-dnd`:** archived by Atlassian in 2023; React 18 strict-mode issues with no fix forthcoming; no React 19 story. Not viable for a new module.

**vs `react-dnd`:** general-purpose drag-and-drop, but the standard HTML5 backend carries the same a11y limitations as native, and the sortable-list use case wants extra backend setup. API surface is heavier than `@dnd-kit`'s `useSortable` hook for the same UX.

**`@dnd-kit` specifics that matter:** keyboard reorder built in (arrow keys, space to pick up/drop), screen-reader announcements, touch/pointer sensors, ~30 KB gzipped, actively maintained. The sortable subpackage exposes `SortableContext` + `useSortable` which the staging grid in Phase E3 will use directly.

**Trade-off acknowledged:** the project has been holding the line on "no dependencies without a clear reason in the PR description". Two packages added (`@dnd-kit/core` is a required peer of `@dnd-kit/sortable`). Justification: brief mandates keyboard reorder, build-it-yourself a11y is a multi-day side quest, dep is maintained.

---

## 2026-05-22 — Asset protocol scope: dynamic per-pick, not static glob

Webview thumbnail previews for picked images (`convertFileSrc(path)` in the Images → PDF staging grid) require the resolved path to fall within Tauri's asset-protocol scope. The narrowest grant we can give the webview is: nothing by default, allow each path the user actually picked. Implemented as:

- [src-tauri/tauri.conf.json](src-tauri/tauri.conf.json): `app.security.assetProtocol = { enable: true, scope: [] }` — the protocol is on, but starts with an empty allowlist.
- [src-tauri/src/asset_scope.rs](src-tauri/src/asset_scope.rs): `allow_image_preview(paths)` Tauri command calls `app.asset_protocol_scope().allow_file(p)` per picked path. Re-validates `.png/.jpg/.jpeg/.webp` server-side because the OS picker's extension filter is advisory (a direct IPC call could bypass it). Registered in [src-tauri/src/tools/mod.rs](src-tauri/src/tools/mod.rs).
- The `tauri` crate needs the `protocol-asset` feature; build script asserts allowlist↔Cargo features parity, so leaving it off wedges the build.

**Why not a static glob (e.g. `**/*.png`)?** Webview-visible to every matching file on disk, not just picked ones. Plan B2's fallback option, but dynamic was confirmed viable so we used it.

**Why not `fs:asset` capability?** The brief used that term loosely; no such permission identifier exists in Tauri 2.x. Asset protocol is a core feature configured under `app.security.assetProtocol`, not a `@tauri-apps/plugin-fs` permission. The capabilities file ([src-tauri/capabilities/default.json](src-tauri/capabilities/default.json)) is unchanged.

**CSP:** still `null`. If tightened later, `img-src 'self' asset: http://asset.localhost` is the entry that lets the rendered `asset:` URLs through.

**Future-tool pattern:** any tool wanting webview-side resource access from user-picked paths should add an `allow_*_preview` command on the same model — extension-validated, allow-file per path. Don't extend the static scope.

Refs: [Tauri 2.x asset protocol scope](https://v2.tauri.app/security/asset-protocol/), [config reference — AssetProtocolConfig](https://v2.tauri.app/reference/config/#assetprotocolconfig).

---

## 2026-05-22 — Tauri plugin baseline: `dialog` + `opener`, wrapped behind `src/lib/system.ts`

`tauri-plugin-dialog` (file picker — `<input type="file">` is a dead-end in Tauri; the webview hides the OS path) and `tauri-plugin-opener` (`revealItemInDir`) are registered in [src-tauri/src/lib.rs](src-tauri/src/lib.rs) with capabilities granted in [src-tauri/capabilities/default.json](src-tauri/capabilities/default.json). **All plugin calls go through [src/lib/system.ts](src/lib/system.ts)** — components stay presentational and Playwright keeps one mock seam. Future tools should extend `system.ts` rather than importing `@tauri-apps/plugin-*` directly. New plugins need their own DECISIONS entry and the narrowest-possible capability grant.

---

## 2026-05-21 — Pdfium is a process-wide singleton

`pdfium-render` guards its native bindings behind a global `OnceCell` — any second `Pdfium::new` blows up (`PdfiumLibraryBindingsAlreadyInitialized`), which kills parallel `cargo test` runs. Use `multitool_core::pdfium::instance() -> Result<&'static Pdfium, AppError>`; never call `Pdfium::new` directly. If a future tool needs a different pdfium configuration, that's a redesign — pdfium can only be configured once per process.

---

## 2026-05-21 — pdfium binary: dynamic-load via `build.rs` download

[multitool-core/build.rs](src-tauri/multitool-core/build.rs) downloads pdfium from <https://github.com/bblanchon/pdfium-binaries> at the pinned `chromium/7763` tag and exports `PDFIUM_LIB_PATH`. The pin must move with `pdfium-render`'s default feature (currently `pdfium_7763`) — bump the two together. `PDFIUM_LIB_PATH` can be set in the environment to bypass the download (offline builds, CI cache, packaged-binary override). Static linking was rejected (needs libclang + prebuilt static pdfium per OS); vendoring was rejected (~30 MB across three platforms).

---

## 2026-05-21 — `AppError`: typed variant only when the UI branches on it

Variants are limited to ones the UI distinguishes meaningfully: `FileNotFound`, `PermissionDenied`, `UnsupportedFormat`, `ProcessingFailed { details }`, `Encrypted`, `Cancelled`. Anything else uses `ProcessingFailed { details }` with the underlying reason in `details`. Adding a variant per failure mode over-fits the enum to one tool.

---

## 2026-05-21 — Heavy deps allowed in `multitool-core`

Pure conversion functions live in `multitool-core` regardless of dep weight (pdfium ~5 MB, `image`, etc.). Keeping them in the Tauri shell instead would break the "testable without Tauri" rule from [ARCHITECTURE §3.1](ARCHITECTURE.md#31-tool-registry-pattern) and re-expose the Windows test-exe launch problem (see "Workspace split" below). The shell stays thin — IPC glue, event emission, and helpers that genuinely need Tauri APIs (e.g. resolving Tauri's app-data dir).

---

## 2026-05-21 — Streaming `on_page` callback in multi-output conversion fns

Pure conversion fns that produce N outputs take a `FnMut(PageOutput) -> Result<(), AppError>` callback plus a `&CancellationToken`, and return only a `JobSummary`. Encoded output for large jobs (a 100-page PDF at 300 DPI in PNG can exceed 500 MB) shouldn't be held in memory. Apply to any 1→N tool (image format conversion across many files, audio segmenting, …); single-output tools return `Result<Output, AppError>` directly.

---

## 2026-05-21 — Test fixtures: real files checked into the repo

Small representative real-world inputs (≤ 20 KB each, ≤ 100 KB total per tool) live in `multitool-core/tests/fixtures/`. Generating fixtures at test time was rejected because not all required artifacts (encrypted PDFs, deliberately-corrupted files) can be produced by our existing deps. If any single fixture exceeds 1 MB, evaluate Git LFS or generate-at-test-time first.

---

## 2026-05-21 — No `default-members` on the cargo workspace

`src-tauri/Cargo.toml` declares no `default-members`. `tauri build` runs `cargo build --bins --features tauri/custom-protocol`; with `default-members = ["multitool-core"]` (a non-Tauri crate) the feature flag misroutes and the build dies on every OS. CI and lefthook pass `cargo test -p multitool-core --all-targets` explicitly because the shell's test exe can't launch on Windows (see "Workspace split" below).

---

## 2026-05-21 — Workspace split: `multitool-core` rlib

`multitool-core` exists because the Tauri shell's test exe fails to launch on the Windows CI runner (`STATUS_ENTRYPOINT_NOT_FOUND` / `0xC0000139`, traced to `ProcessPrng` in `bcryptprimitives.dll` imported transitively via `tauri → getrandom 0.3.4`). Consequence: **the shell has no test lane** — everything worth testing must live in `multitool-core`. CI and lefthook both run `cargo test -p multitool-core --all-targets`.

---

## 2026-05-21 — `pnpm.packageExtensions` for the vitest vite peer

`package.json` injects `@types/node` into vitest's peer set so TypeScript sees a single `Plugin` type and can accept `react()` / `tailwindcss()` in the vite config. If a future devDep reintroduces a no-`@types/node` vite peer, expect the same diagnostic (two `Plugin` types) and the same fix.
