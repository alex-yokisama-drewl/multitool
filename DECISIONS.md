# Decisions

Choices, caveats, and recipes that affect future work — patterns we must keep following, or non-obvious workarounds we want to make sure don't get reverted. Newest first. Product scope: [ASSIGNMENT.md](ASSIGNMENT.md). Architecture overview: [ARCHITECTURE.md](ARCHITECTURE.md).

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
