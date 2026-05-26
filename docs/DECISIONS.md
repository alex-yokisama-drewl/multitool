# Decisions

Choices, caveats, and recipes that affect future work — patterns we must keep following, or non-obvious workarounds we want to make sure don't get reverted. Newest first. Architecture overview: [ARCHITECTURE.md](ARCHITECTURE.md). Plans and ideas: [plans/BACKLOG.md](plans/BACKLOG.md).

**Keep entries short.** Only spend more than a few lines when there's a real risk someone may want to revert the decision and re-do the work. For choices between equally valid options, or where the alternative obviously breaks the app, a sentence or two is enough.

---

## Image Crop: backend-served format set, source-format preservation, reusable clamp

The Image Crop tool (commits `83c83f1…`) is the second image tool and the trigger for making the encodable-raster-format set a single, backend-owned source of truth:

- **`RasterFormat` is the one source of truth, served over IPC.** The image-format converter's old `TargetFormat` enum became [`multitool_core::image::RasterFormat`](../src-tauri/multitool-core/src/image/raster_format.rs) (PNG/JPEG/WebP/BMP/TIFF — the formats we can both decode *and* encode). `TargetFormat` is now a type alias, so the converter is unchanged. The `supported_raster_formats` Tauri command (in [`src-tauri/src/system.rs`](../src-tauri/src/system.rs)) serializes the variants' metadata; the frontend reads it via the memoized [`getRasterFormats()`](../src/lib/imageFormats.ts) and stops hard-coding format lists (the converter's output dropdown + alpha gate, the crop picker filter). **Adding a new encodable format = add the variant + match arms in `raster_format.rs`; the picker filter and dropdown follow automatically.** Don't re-introduce a literal extension array on the TS side.
- **Crop preserves the source format by the file *extension*, not the sniffed bytes.** `crop_one` decodes EXIF-oriented bytes (bytes win, so a renamed file still decodes correctly) but re-encodes to `RasterFormat::from_extension(source_ext)`. So PNG bytes named `.jpg` come out as JPEG — "keep the extension, re-encode to match it". Output is `{stem}_cropped.{ext}` next to the source via `unique_path`. JPEG re-encodes at a fixed quality 90 (no knob — geometry tool; chain through the converter for quality control).
- **Multi-frame TIFF is rejected before decode via a hand-rolled IFD walker.** Neither `image` nor the `tiff` crate exposes a top-level frame count, and `DynamicImage::from_decoder` silently returns only the first frame — which would drop pages on a crop. `tiff_frame_count` walks the classic-TIFF IFD linked list (`II`/`MM` + magic 42, then count/entries/next-offset per IFD); BigTIFF or any malformed/truncated chain returns `None` and falls through to the normal decoder. >1 IFD → `UnsupportedFormat`.
- **The clamp is a reusable primitive, not buried in the tool.** [`CropRect::clamp_to(w,h) -> Option<PixelRect>`](../src-tauri/multitool-core/src/tools/image_crop/convert.rs) is the source of truth for "what rect actually gets cropped": zero-size dims forced to 1px, partial overflow clamped to the image intersection, no intersection → `None` (caller errors). `CropRect.x/y` are **signed** so a frame dragged off-canvas survives the wire. The frontend clamps too (for frame feel), but the backend clamp is authoritative — a future tool can call `crop_one` directly and trust it.
- **Crop-tool e2e needs a real image, not a stub URL.** The frame editor gates on the preview `<img>`'s `onLoad` (it reads `naturalWidth/Height`). The e2e [`mocks/system.ts`](../tests/e2e/mocks/system.ts) `imageAssetUrl` therefore returns a real 1×1 PNG **data URL**, not the old `mock-asset://` stub that 404s — otherwise Crop never enables in a plain Chromium. Pure-DOM geometry (drag math, aspect lock) is unit-tested in [`cropGeometry.test.ts`](../src/tools/image-crop/cropGeometry.test.ts), so the e2e only proves the happy path is wired.

---

## Audio Extractor: per-track ffmpeg call, asymmetric `_audio[_N]` naming

The Audio Extractor (commits `15961cd…e0e8ad2`) takes a single video in and writes one MP3 per audio track. Two decisions worth pinning so they don't get "optimized" later:

- **One ffmpeg call per track**, not a single call with N `-map ... outN.mp3` outputs. ffmpeg's multi-output form decodes the source once and writes every track in a single run — measurably faster on large sources. Rejected because: it reuses the existing single-output `convert` shape unchanged, mid-track progress is just `out_time_us / duration` instead of demuxing a multi-output progress stream, and between-track cancellation is a top-of-loop `is_cancelled()` check instead of mid-child surgery on a child still owed N output files. The decode-N-times cost is negligible for an offline tool that runs occasionally. If "extract 8 tracks from a 4-hour rip" ever shows up as a real wait, the refactor is self-contained inside `audio_extractor`.
- **Asymmetric naming: `<stem>_audio.mp3` for single-track, `<stem>_audio_<1-based>.mp3` for multi-track.** Single-track sources are the 95% case and `_audio_1.mp3` reads as noise when there's only one track. The 1-based index in filenames intentionally diverges from the 0-based ffmpeg `-map 0:a:<i>` selector — users count from 1, ffmpeg counts from 0; the boundary is in `derive_output_path`.
- **`probe_audio_stream_count` parses `ffmpeg -i` stderr.** Same trick as `probe_duration_secs` and same reason — no ffprobe bundled. Anchors on two substrings on the same line (`Stream #` prefix + `: Audio:` marker) so banner prose mentioning "Audio" doesn't get counted.
- **MP3 V2 VBR (~190 kbps) via `libmp3lame -q:a 2`.** Matches the "baked recipe, no UI knob" ethos of the other converters. V2 over CBR for quality-per-byte; V2 over V0 (~245k) for smaller files. The `mp3lame-encoder` crate is **not** used here even though we have it — going through ffmpeg keeps the pipeline single-tool (decode any video container ffmpeg knows + encode in one process); the upstream codec is the same libmp3lame either way.

---

## Video stack: bundled ffmpeg sidecar (eugeneware GPL build), baked codec recipes

The Video Format Converter (commits `601b48c…c27b34c`) is the first video tool. Decisions worth keeping:

- **Sidecar, not in-process bindings.** ffmpeg ships as a subprocess via [`multitool_core::ffmpeg::run`](../src-tauri/multitool-core/src/ffmpeg.rs), not via `ffmpeg-next` libav bindings. Sidecar adds ~50–80 MB to the installed bundle per OS-arch (acceptable for a learning project) but keeps the CI surface trivial — no system libav headers, no per-OS dev-deps. `ffmpeg-next` would have required `apt-get install libav*-dev` / `brew install ffmpeg` / `cargo vcpkg build` on every builder, same trap as HEIC (see [plans/BACKLOG.md](plans/BACKLOG.md)). Spawn/progress drainage/stderr capture all go through `Command::spawn` + `BufRead` on a dedicated thread; Windows needs `CREATE_NO_WINDOW = 0x0800_0000` to avoid a cmd.exe flash on every spawn.
- **eugeneware/ffmpeg-static at `b6.1.1` for the binary.** Single source covers all five target platforms (linux x64/arm64, darwin x64/arm64, win x64) with bare-binary downloads — no .tar.xz/.zip/.7z extraction logic. Rejected: BtbN/FFmpeg-Builds (no macOS at all; evermeet.cx has no darwin-arm64, so we'd have needed three sources) and BtbN's static binaries at ~160 MB vs eugeneware's leaner ~50–80 MB. The `b6.1.1` tag on Linux actually ships ffmpeg 7.0.2 binaries — johnvansickle's static build re-distributed; the tag is a re-distribution marker, not the ffmpeg version. **If the pin moves, update both build scripts together** (same rule as pdfium).
- **No `ffprobe` bundled — parse `ffmpeg -i` stderr for duration.** Saves another ~50–80 MB per installed bundle. The `Duration: HH:MM:SS.cc` line format has been stable in ffmpeg for over a decade; parser lives in `ffmpeg::probe_duration_secs`.
- **Baked codec recipes**, no per-format quality knobs in v1. Matches the "format dropdown only" ethos of the audio + image converters:
  - **mp4**: H.264 (libx264) CRF 23 `-preset medium` + AAC 128k, `-vf scale=trunc(iw/2)*2:trunc(ih/2)*2`, `-pix_fmt yuv420p`.
  - **webm**: VP9 (libvpx-vp9) CRF 32 `-b:v 0` (constant-quality) `-row-mt 1` + Opus 96k, same scale filter.
  - **mkv**: stream copy, no re-encode.
- **`-vf scale=trunc(iw/2)*2:trunc(ih/2)*2` is load-bearing.** Both libx264 (in 4:2:0) and libvpx-vp9 reject odd source dimensions on most configurations with "height not divisible by 2" (real failure observed on a 1062×1043 source). The scale filter rounds down at most 1 px per axis — the standard fix. Don't drop it.
- **GPL licensing accepted for the bundle.** eugeneware's binary is a GPL ffmpeg build (libx264 is GPL). Fine for a learning-project capped at 0.x. **Don't propose a publishing pipeline that pretends the bundle is LGPL** — switching to LGPL would drop H.264 encode entirely, which kills mp4 output.

Cancellation: between files via `cancel.is_cancelled()` check at the top of each iteration; mid-encode via `child.kill()` from inside [`ffmpeg::run`](../src-tauri/multitool-core/src/ffmpeg.rs). On any error (cancel or non-zero exit) the in-flight partial output file is unlinked — half-written mp4/webm files are useless. Already-written outputs from prior files stay on disk.

---

## Audio Trimmer: source-format-preserving, browser-side preview, shared `audio_codecs` module

The Audio Trimmer (commits `2cab704…cc136e6`) is the second audio tool and the trigger for hoisting decode + encode out of the converter:

- **Shared codec module.** Decode + encode primitives moved to [`multitool_core::audio_codecs::{decode, encode}`](../src-tauri/multitool-core/src/audio_codecs/). Both tools (and any future audio tool — compress, concat) depend on this surface. The converter's `audio_format_converter::convert` keeps `TargetFormat` / `ChannelMode` / `Opts` / `convert_one` but routes decode and the four encoders through the shared module. **Public API unchanged.**
- **Output preserves source format.** Picker is restricted to `wav / mp3 / flac / ogg / oga` — the four formats with available encoders. m4a / aac / aiff / caf / mkv / webm are Symphonia-decodable but not round-trippable without a transcode; users chain through the Audio Format Converter for that case. `Opts` carries no encoder knobs (bitrate / quality / bit depth); per-format defaults live as constants on `audio_trimmer::job` (WAV 16-bit, MP3 192 kbps, OGG q=5.0, FLAC level = no-op).
- **Browser-side preview.** [`src/lib/audioPreview.ts`](../src/lib/audioPreview.ts) fetches via the Tauri asset protocol (`audioAssetUrl` → per-pick `allow_media_preview` scope grant), decodes through Web Audio's `decodeAudioData`, and serves *both* the waveform peaks (1000 bins, min/max per bin, mono-mixed) and the `AudioBufferSourceNode + GainNode` preview chain. Fade preview uses `linearRampToValueAtTime` to approximate the encoder's gain envelope — accurate enough for "did I pick the right region" iteration; no Rust round-trip per Preview click. **Trade-off:** decodeAudioData holds the full PCM in memory in the browser; fine for v1's tiny-to-moderate files. Streaming peaks + a Rust-side waveform command is a follow-up if hour-long files become a use case.
- **Fade UX: checkbox + fixed duration.** The UI exposes Fade-in / Fade-out as checkboxes that toggle `FADE_PRESET_MS = 1000`. Rust `Opts` keeps `fade_in_ms` / `fade_out_ms` as `u32` so unit tests can hit edge cases (zero, equal-to-window, overlap-clamp).
- **Silent range clamp.** Setters enforce `0 ≤ start ≤ end − 1 ≤ durationMs − 1`. No "End must be after Start" alert; instead the input silently snaps to the closest legal value. Any start/end/fade change while preview is playing stops the preview (gain envelope was baked in at schedule time).
- **Asset-protocol scope generalized.** `allow_image_preview` → `allow_media_preview` with `IMAGE_EXTS + AUDIO_EXTS` allowlists. The dynamic-per-pick policy ("Asset protocol scope" entry below) extends to media families without growing the command surface. Future video preview tools follow the same shape.

---

## Audio stack: Symphonia for decode, format-specific encoders, no resampler

The Audio Format Converter (commits `8351b63…08eb569`) intentionally splits decoder and encoder concerns rather than picking a single all-in-one crate.

- **Decode**: [`symphonia`](https://github.com/pdeljanov/Symphonia) for everything except FLAC. Pure Rust, broad container/codec coverage (mp3 / aac / alac / vorbis / wav / aiff / caf / isomp4 / mkv / webm), decode-only.
- **FLAC decode**: [`claxon`](https://github.com/ruuda/claxon) routed via a `fLaC` magic-byte sniff in `decode_to_pcm`. Symphonia 0.6's FLAC demuxer is strict about STREAMINFO's `total_samples` matching the demuxed frame count, and our own `flacenc`-produced output trips that check ("unexpected end of file") even though `ffprobe` reads the bytes cleanly. claxon — the de-facto Rust FLAC decoder, also used by flacenc's own integrity tests — handles both flacenc and ffmpeg FLACs reliably.
- **Encoders**: one crate per output. WAV via [`hound`](https://github.com/ruuda/hound) (pure Rust), FLAC via [`flacenc`](https://github.com/yotarok/flacenc-rs) (pure Rust), MP3 via [`mp3lame-encoder`](https://github.com/DoumanAsh/mp3lame-encoder) (vendored LAME 3.100), OGG Vorbis via [`vorbis_rs`](https://github.com/ComunidadAylas/vorbis-rs) (vendored libogg + libvorbis with aoTuV/Lancer patches).
- **Sample rate**: **passthrough only** in v1. No resampler. MP3 inputs at rates outside LAME's accepted set (`8/11.025/12/16/22.05/24/32/44.1/48 kHz`) are rejected with a clear per-file message; the orchestrator turns them into `Progress::Skipped` events.
- **Channels**: simple count, not a layout. `apply_channel_mode` does equal-weight averaging for downmix and L=R for mono→stereo upmix. Layout-aware mixing (5.1 center channel, surround weighting) needs a `Channels` enum threaded through symphonia/claxon/encoders — follow-up.

`flac_compression_level` is wired through the IPC shape but is **currently a no-op** because `flacenc` 0.5 has no single compression knob — only fine-grained `subframe_coding` / `stereo_coding` blocks. Keeping the field forward-compatible so we don't churn the wire shape if/when a level → fine-knob mapping lands.

Cancellation is between files only in v1. Mid-file cancel needs the encoders switched to streaming chunked I/O (LAME + Vorbis support it; hound + flacenc would need a per-frame loop).

---

## Audio: `mp3lame-sys` requires GNU autotools on Unix (macOS CI brew step)

`mp3lame-sys` (transitive of `mp3lame-encoder`) builds LAME 3.100 with **GNU autotools on Unix** (`autoconf`, `automake`, `libtool`) and `cc` on Windows.

- **ubuntu-latest** already ships autoconf/automake/libtool via `build-essential` — no extra step needed.
- **macos-latest** does NOT ship them by default — added a `brew install autoconf automake libtool` step to both [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) and [`.github/workflows/release.yml`](../.github/workflows/release.yml) (right after the Linux deps block).
- **windows-latest** uses cc — no extra step.

Don't drop the brew step. `vorbis_rs` and its `aotuv_lancer_vorbis_sys` core build via cc-rs only (no autotools), so this is mp3lame-specific.

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
