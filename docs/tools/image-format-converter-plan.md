# Image Format Converter — Build Plan

> **Living document.** Update this file as you work — flip checkboxes, capture mid-build decisions, amend phases when scope shifts. The plan you started with is rarely the plan you finish with; the value of this file is that mid-build hand-offs (across sessions or contributors) start from current truth, not stale aspiration. Architectural choices that emerge mid-build (e.g., "we ended up extracting an EXIF helper to `multitool-core::image`") belong in [`DECISIONS.md`](../../DECISIONS.md), not here.
>
> **Deleted when the tool ships**, alongside the assignment brief [`image-format-converter.md`](image-format-converter.md). Tools are meant to be self-describing in code.

## References

Reuse the patterns rather than re-deriving them.

- **Playbook:** [`docs/adding-a-tool.md`](../adding-a-tool.md) — canonical step list. This plan is the per-tool specialization of it.
- **Streaming N-output reference:** [`multitool-core/src/tools/pdf_to_images/`](../../src-tauri/multitool-core/src/tools/pdf_to_images/) — `convert.rs` + `job.rs` + `writer.rs` + shell `mod.rs` + the TS wrapper [`src/lib/tools/pdfToImages.ts`](../../src/lib/tools/pdfToImages.ts). Closest analogue to this tool (1-job → N-files-on-disk, streaming `on_*` callback).
- **Multi-file staging UX reference:** [`src/tools/images-to-pdf/ImagesToPdf.tsx`](../../src/tools/images-to-pdf/ImagesToPdf.tsx) — picker + staging list + add-more + per-row remove. We drop reorder (order doesn't matter for this tool) but keep the rest.
- **EXIF orientation handling:** [`images_to_pdf/convert.rs::decode_with_orientation`](../../src-tauri/multitool-core/src/tools/images_to_pdf/convert.rs) — copy the approach (and the `rotated.jpg` fixture).

## Decisions locked in (2026-05-22)

Captured from the kickoff conversation; restated here so the plan is self-contained.

- **Output formats** — PNG / JPEG / WebP / BMP / TIFF. No GIF/SVG/AVIF/HEIC output.
- **Input formats** — PNG, JPEG, WebP, BMP, TIFF, GIF (first frame for animated), ICO, TGA, PNM (PBM/PGM/PPM), QOI (all free via `image` crate features); **SVG via `resvg`** (rasterized — pure Rust, no native deps).
- **Alpha handling default** — `flatten-white`.
- **Batch error policy** — **skip + continue**. Per-file failures land in an error list surfaced at job end; the overall job result is `Ok`. Cancellation and structural I/O failures (`FileNotFound`) still abort.
- **Animated GIF** — first frame + warning. A dedicated "decompose GIF into frames" tool can be a separate future scope.

## Phases

Each `[ ]` is one commit. Stay scoped — if a step grows past ~200 LoC of diff, split it.

### Phase A — Pure conversion logic (`multitool-core`)

- [ ] **A0** · `chore(deps): widen image features + add resvg for svg input` — update `multitool-core/Cargo.toml`'s `image` features to include `bmp`, `tiff`, `gif`, `ico`, `tga`, `pnm`, `qoi` (in addition to the existing `png`, `jpeg`, `webp`). Add `resvg` (which pulls in `usvg` + `tiny-skia` transitively — keep `default-features = false` and only enable what we need; check the resvg docs for the lightest set that still rasterizes). No behavior change yet. Verify with `cargo build -p multitool-core`. **Commit gate:** the workspace clippy lane (`cargo clippy --workspace --all-targets -- -D warnings`) must stay green even with the new transitive surface.
- [ ] **A1** · `feat(image-format-converter): scaffold module + fixtures` — `multitool-core/src/tools/image_format_converter/{mod.rs, convert.rs, job.rs}` with `Opts`, `TargetFormat`, `AlphaHandling`, `SvgRasterSize`, `FileOutcome` (Success / Failed / Warning variants), `FileProgress`, `JobSummary` types. Stub both `convert_one` and `run_job` returning `Err(ProcessingFailed)`. Add fixtures under `multitool-core/tests/fixtures/image_format/` (re-use Images → PDF fixtures where possible: `red.png`, `blue.jpg`, `green.webp`, `rotated.jpg`; add `tiny.bmp`, `tiny.tif`, `static.gif`, `animated.gif`, `alpha.png`, `tiny.ico`, `tiny.tga`, `tiny.pgm`, `tiny.qoi`, `tiny.svg`, `garbage.bin`). Total ≤ 100 KB per the [test-fixtures decision](../../DECISIONS.md). Register module in `tools/mod.rs`.
- [ ] **A2** · `feat(image-format-converter): convert_one for raster inputs` — implement the pure single-file converter `convert_one(input_bytes, source_ext, opts) -> Result<EncodedFile, AppError>` for the raster formats (PNG/JPEG/WebP/BMP/TIFF/GIF/ICO/TGA/PNM/QOI → PNG/JPEG/WebP/BMP/TIFF). EXIF orientation honored on input (port `decode_with_orientation` from `images_to_pdf` — likely a shared helper, see [Phase F](#phase-f--post-ship-generalization-audit)). One unit test per output format × representative input pairing. Quality clamp tests.
- [ ] **A3** · `feat(image-format-converter): alpha handling matrix` — implement `alpha_handling`: `preserve` returns `ProcessingFailed` when target lacks alpha and the image has non-trivial alpha; `flatten-white` / `flatten-black` composite RGBA onto solid background before encoding. Tests cover all three modes against `alpha.png → JPEG` and the no-op cases (alpha-having target, fully-opaque input).
- [ ] **A4** · `feat(image-format-converter): svg input via resvg` — extend `convert_one` to detect SVG (by source extension) and route to a `rasterize_svg(bytes, opts.svg_raster_size) -> RgbaImage` path using `usvg::Tree::from_data` + `resvg::render` into a `tiny_skia::Pixmap`. Convert pixmap → `image::RgbaImage` → feed into the existing encode pipeline (alpha handling still applies). Size policy: `natural` honors intrinsic dims (fall back to viewBox; `UnsupportedFormat` if both missing); `longest-edge-px:N` scales preserving aspect ratio. Tests: tiny SVG rasterizes at intrinsic, at longest-edge=512, missing viewBox → `UnsupportedFormat`.
- [ ] **A5** · `feat(image-format-converter): batch orchestrator with skip+continue` — `job.rs::run_job` reads each input file's bytes off disk, calls `convert_one`, on success resolves the output path via `unique_path` + atomic-rename writes, on failure pushes `{ path, error }` to a skipped list. Cancel checkpoint between files (`Cancelled` aborts the whole job regardless of progress). `FileNotFound` on a single file is treated as a per-file skip (consistent with "skip + continue"); orchestrator-level failures (output dir un-writable for the entire batch) abort. Progress callback fires for every file with `Started → Done | Skipped`. Returns `JobSummary { success_count, skip_count, skipped: Vec<{path, error}>, duration }`. Tests: happy 3-file batch, mid-batch un-decodable file is skipped (job result is `Ok`, summary contains the failure), missing input file → skipped, cancel mid-batch yields `Cancelled` and no half-written files, output-name collision lands on `name (1).ext`, same-format re-encode preserves original via `unique_path`.
- [ ] **A6** · `feat(image-format-converter): per-file warnings (animated GIF, etc.)` — extend the progress callback's payload so warnings surface without failing the file. Animated GIF emits `"animated; converted first frame only"`. SVG with text nodes emits `"SVG references fonts; text may not render"`. SVG with external `<image>` href emits `"SVG references external resources; not loaded"`. Tests against `animated.gif` and a small SVG-with-text fixture.

### Phase B — Tauri shell + IPC wrapper

- [ ] **B1** · `feat(image-format-converter): tauri command shim + register_commands` — `src-tauri/src/tools/image_format_converter/mod.rs` registers the JobId, dispatches `job::run_job` on `spawn_blocking`, threads the `on_file` callback through `app.emit("tool:progress", …)`, emits `tool:complete` / `tool:error` after the join, returns `Result<JobResult, AppError>`. Append the `#[tauri::command]` fn to `register_commands` in `src-tauri/src/tools/mod.rs` (one-line edit to a shared file — the only one allowed by [`adding-a-tool.md`](../adding-a-tool.md)).
- [ ] **B2** · `feat(image-format-converter): typescript IPC wrapper + tests` — `src/lib/tools/imageFormatConverter.ts` mirroring [`pdfToImages.ts`](../../src/lib/tools/pdfToImages.ts): internal `crypto.randomUUID()` JobId, filtered progress listener, `try { … } finally { unlisten(); }`, `AbortSignal` → `cancel_job`. Vitest covers: right command name + payload shape, progress filtered by JobId, listener cleanup on both success and error paths, abort signal triggers `cancel_job`, error envelope round-trips.

### Phase C — Frontend tool module

- [ ] **C1** · `feat(image-format-converter): tool scaffold (index, types, empty component)` — `src/tools/image-format-converter/{index.ts, types.ts, ImageFormatConverter.tsx}`. `index.ts` exports `Tool` metadata. Component renders a stub header only; no IPC yet.
- [ ] **C2** · `feat(image-format-converter): staging list + multi-pick + add/remove` — adapt the Images → PDF staging list, **stripping the reorder logic** (no `@dnd-kit` — order is irrelevant). File picker via `src/lib/system.ts` (extend if a multi-image picker variant is needed). Per-row remove. "Add more" appends. Thumbnails optional in this pass — only add if the asset-protocol scope grant from the Images → PDF tool is reusable as-is. If thumbnails need new plumbing, defer to a separate commit.
- [ ] **C3** · `feat(image-format-converter): options form` — `target_format` radio/dropdown, `jpeg_quality` slider (shown when JPEG selected), `webp_quality` slider (shown when WebP selected), `alpha_handling` radio (shown when target lacks alpha), `svg_raster_size` controls (shown only when at least one staged input has `.svg` extension). Forwarded to the wrapper as a typed `Opts` object. Vitest smoke test: each option renders with its default, conditional fields appear under the right conditions, and changing them updates the payload passed to the wrapper.
- [ ] **C4** · `feat(image-format-converter): progress, success, error, cancel states` — state machine `idle → staged → running → done` (no terminal `error` — skip+continue means the job result is always `Ok` unless cancelled or hit an unrecoverable orchestrator error). Per-file progress text. `done` shows `N converted, M skipped` with an expandable list of skipped files and reasons. Per-file warnings render as inline tags on the success rows. Cancel button aborts via `AbortController`; staged list preserved for retry.
- [ ] **C5** · `feat(image-format-converter): register tool + dashboard tile` — import + array entry in [`src/tools/registry.ts`](../../src/tools/registry.ts), update [`src/app/Dashboard.test.tsx`](../../src/app/Dashboard.test.tsx) to assert the new tile (the test belongs to the registry contract per [`adding-a-tool.md`](../adding-a-tool.md)).

### Phase D — End-to-end + cross-OS

- [ ] **D1** · `test(image-format-converter): playwright happy path` — one e2e lane: dashboard → tile → pick → stage → choose JPEG → convert → success. Mock at `src/lib/tools/imageFormatConverter.ts` boundary via the existing `tests/e2e/mocks/` + Vite alias pattern. Failure paths stay at the Vitest level.
- [ ] **D2** · `ci: smoke CI sweep on linux/macos/windows` — if the conversion features touch native deps in ways the existing image-using tools didn't (unlikely with PNG/JPEG/WebP/BMP/TIFF on `image` defaults+webp+tiff, but check), run CI early to catch platform regressions before stacking more work. This may collapse into the PR for B1 / D1 if no new native code lands.

### Phase E — Ship

- [ ] **E1** · Run the [pre-PR checklist](../../CLAUDE.md#per-pr-checklist) end to end: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p multitool-core --all-targets`, `pnpm lint`, `pnpm typecheck`, `pnpm format:check`, `pnpm test`, `pnpm tauri build --no-bundle`, `pnpm test:e2e`. **Run `pnpm format:check` in the same batch as your pre-commit gates** — lefthook prettier gates fail commits if files aren't formatted.
- [ ] **E2** · Open PR against `master`. PR description: what / why / how-tested, with a screenshot of the tool tile and the form. `gh run watch` until green on all three OSes; `gh run view --log-failed` if anything regresses.
- [ ] **E3** · After merge: delete `docs/tools/image-format-converter.md` and `docs/tools/image-format-converter-plan.md`; remove the "Image format converter" follow-up line from [`ASSIGNMENT.md §7`](../../ASSIGNMENT.md#7-nice-to-haves--follow-ups).

### Phase F — Post-ship generalization audit

> Three tools now share the codebase (PDF → Images, Images → PDF, Image Format Converter). Two tools is coincidence; three is a pattern. This phase is **deliberately after ship** — premature extraction is a worse trap than mild duplication, and we don't know the shape of the third use until it's running.

Run this as a separate exploratory PR. Do not block the tool's ship on it.

- [ ] **F1** · Audit duplicated code across the three tools. Likely candidates (verify, don't assume):
  - **EXIF-orientation-aware image decode** — `decode_with_orientation` in `images_to_pdf/convert.rs` and the Phase A3 port. If both look identical, extract to `multitool_core::image::decode_oriented`.
  - **Multi-file staging list UI** — Images → PDF and Image Format Converter both render an "add files / remove row" list. If both diverge only in row content, extract a `<StagingList>` component into `src/components/`. Beware: Images → PDF adds reorder via `@dnd-kit`; the converter does not. Extracting prematurely may force one to carry the other's complexity.
  - **`on_*` streaming callback boilerplate** — three tools now wire `on_page` / `on_file` through `spawn_blocking` + `app.emit("tool:progress", …)`. There may be a small helper that takes `(app, job_id, event_name, run)` and abstracts the dispatch.
  - **Job orchestrator boilerplate** — `unique_path` + atomic write + cancel checkpoint + warnings emission. If the orchestrators look like three variations of the same dance, lift a `run_streaming_job` helper.
  - **Quality/clamp option patterns** — DPI clamp in PDF → Images, quality clamps here. If the shape repeats, a small `clamped::<u32, MIN, MAX>` helper might be worth more than three copies of the same `max(…).min(…)`.
- [ ] **F2** · For each candidate: decide **extract** vs **leave duplicated**. The bar is concrete — extract only if (a) the bodies are nearly identical, (b) future tools are likely to want the same surface, and (c) extraction doesn't force divergent tools to grow optional knobs. If you skip an extraction, leave a one-liner in [`DECISIONS.md`](../../DECISIONS.md) under "Generalization audit YYYY-MM-DD" so future-you doesn't re-audit the same thing.
- [ ] **F3** · For each extraction taken: add a [`DECISIONS.md`](../../DECISIONS.md) entry naming the shared surface and the rule for using it (so a fourth tool consumes from the shared surface instead of re-deriving). Update [`docs/adding-a-tool.md`](../adding-a-tool.md) → "Shared surfaces" table to list the new helper(s).

---

## Notes & mid-build addenda

> Append notes here as the build progresses. Things like: "A3 split into A3a / A3b because alpha handling tests grew", "WebP encoding lossless mode needs feature `webp-encoder` not just `webp`", "switched the row-remove animation off because the `<StagingList>` extraction made it cheaper to drop", etc. Better to over-share than to leave a follow-on session guessing.
