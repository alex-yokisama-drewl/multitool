# Decisions

Choices, caveats, and recipes that affect future work — patterns we must keep following, or non-obvious workarounds we want to make sure don't get reverted. Newest first. Architecture overview: [ARCHITECTURE.md](ARCHITECTURE.md). Plans and ideas: [plans/BACKLOG.md](plans/BACKLOG.md).

**Keep entries short.** Only spend more than a few lines when there's a real risk someone may want to revert the decision and re-do the work. For choices between equally valid options, or where the alternative obviously breaks the app, a sentence or two is enough.

---

## Settings memory: hand-rolled JSON document, frontend-owned schemas

User settings (last-used directory, remembered tool options) persist in a versioned JSON document in Tauri's app-config dir. Two commands — [`load_settings` / `save_settings`](../src-tauri/src/settings/mod.rs) — over pure logic in [`multitool_core::settings`](../src-tauri/multitool-core/src/settings.rs); the app-config path resolves through [`crate::fs::settings_path`](../src-tauri/src/fs/mod.rs), the first helper that module earned.

Rejected `tauri-plugin-store` (a new dep + capability grant for a generic key-value API wider than we need) and webview `localStorage` (not an inspectable file, wiped with webview data, puts durable state outside the Rust boundary). Follows the `file_size` precedent: a narrow command beats a generic plugin.

Three properties worth not reverting:

- **The document is an opaque string-keyed blob.** Rust validates the envelope — version, object shape, a size ceiling — and nothing inside it. Every schema lives in the frontend ([`src/lib/prefs.ts`](../src/lib/prefs.ts) + each tool's `prefs.ts`). This is what keeps the registry contract intact: a new tool declares its own prefs in its own folder and no shared Rust code changes. Typing the entries in Rust would make every option change a two-sided edit.
- **Loading never fails.** Missing, corrupt, oversized or foreign-version all resolve to defaults, with a `LoadOutcome` for the shell to log. Losing preferences is an annoyance; refusing to start is a bug. Saving is atomic (temp + rename) because writes are debounced while the user is still clicking.
- **Nothing is written before hydration succeeds.** An unhydrated store is an in-memory session store. Otherwise a failed load would clobber real settings with defaults on the first interaction — and it also keeps Vitest and plain-browser runs from attempting IPC.

Every stored value is sanitized per field on read, falling back to the default rather than forwarding junk to Rust; one bad option doesn't discard the good ones beside it. Range-clamping sanitizers (`numberIn`) are only for slider-backed options. **For a free-typed number field use `finiteNumber`** — clamping on read fights the user mid-keystroke, since typing `7` on the way to `700` would snap the field to the minimum. Those options clamp in Rust, where they already did.

**Where the file is.** `settings.json` under Tauri's `app_config_dir()` for identifier `com.drewl.multitool` — `~/.config/com.drewl.multitool/` on Linux, `~/Library/Application Support/com.drewl.multitool/` on macOS, `%APPDATA%\com.drewl.multitool\` on Windows. It is plain pretty-printed JSON, safe to read or hand-edit: anything the sanitizers reject falls back to a default.

**There is no reset-to-defaults UI.** Deleting that file is the escape hatch — the next launch loads defaults and rewrites it. Backlogged as an affordance; until then it is the answer to "my settings are stuck in a weird state".

**Bump `SETTINGS_VERSION` only for a change the frontend's sanitizers cannot absorb.** Field-level drift — an option added, removed, or whose variants changed — is already handled per field on read, and bumping throws away every unrelated preference with it.

**Gotcha for future shared modules under `src/lib/`:** import anything the e2e lane stubs by its aliased path (`@/lib/settings`), not relatively. `vite.config.ts` swaps mocks by exact specifier, so `./settings` slips past the alias and reaches the real IPC wrapper.

**Writes are debounced, which leaks into tests.** `flushPrefs()` forces the pending write and awaits it; `resetPrefsForTest()` drops in-memory state and any scheduled write. A Playwright spec that drops the page has to outlast the debounce window first (`WRITE_DEBOUNCE_MS` in [`src/lib/prefs.ts`](../src/lib/prefs.ts)) — see [`tests/e2e/settings-memory.spec.ts`](../tests/e2e/settings-memory.spec.ts).

---

## Directory memory is keyed by picker media kind

The last-used directory is remembered per media kind — `pdf` / `image` / `audio` / `video` — in [`src/lib/lastDirectory.ts`](../src/lib/lastDirectory.ts), applied in [`src/lib/system.ts`](../src/lib/system.ts).

Not one global directory: pickers are extension-filtered, and that filter decides whether a remembered folder is any use. Reopening the video trimmer where PNGs were last picked shows an empty dialog — worse than no memory. Not per-tool either: within a media kind the directory is nearly always the same, and per-tool memory would make someone navigate to the same folder three times to crop, resize and compress one batch of photos.

**The key is not `ToolCategory`.** Images → PDF is category `pdf` but picks images, so category would send it to the wrong folder. If a future tool's picker doesn't match its category, follow the picker.

A kind used for the first time falls back to the last directory picked anywhere. A remembered path is checked with the `directory_exists` command before it reaches the dialog — drives get unplugged and folders deleted between sessions.

Both halves live in `system.ts`, so a new tool inherits this by calling an existing picker.

---

## What tool options are remembered, and what isn't

Remember a setting if it expresses **how the user works**; don't if it is derived from **the file in front of them**. Formats, quality dials, page size, fit mode and the fade toggles stick. Crop rects, trim windows and resize dimensions don't.

Two consequences that look like bugs if you don't know the rule:

- **Lock toggles persist as a boolean only** (Image Crop proportions, Image Resize aspect). The ratio is re-derived from whichever image just loaded, so the lock never drags one image's shape onto the next.
- **Image Resize `background` is deliberately not remembered.** It is defaulted per picked file from whether the source format carries alpha, so a stored value would be clobbered on every pick. It is content-derived, not a preference.

Preview volume is app-scoped ([`src/lib/playbackPrefs.ts`](../src/lib/playbackPrefs.ts)), not per-tool: the Video Trimmer and Video Compress show the same preview, and turning the sound down means it for both.

`reset()` ("Convert another") clears the job and leaves options alone. Resetting them there would undo the user's settings on the most common path through a tool.

---

## Image Compress: per-source strategy, slider default 0.8, EXIF stripped

The Image Compress tool re-encodes via a per-source strategy keyed on the file's **extension** (matching the [`image_crop`](../src-tauri/multitool-core/src/tools/image_crop/) "extension wins for output, bytes win for decode" rule):

| Source ext             | Output ext              | Encoder       |
| ---------------------- | ----------------------- | ------------- |
| `jpg` / `jpeg`         | source spelling kept    | `image` JPEG  |
| `webp`                 | `webp`                  | `webp` (lossy)|
| `png` / `bmp` / `tif(f)` | `webp`                | `webp` (lossy)|

- **JPEG / WebP preserve the source format.** Aligns with the project's "output format matches input unless explicitly stated otherwise" philosophy (Crop / Resize / Trimmer all preserve format).
- **PNG / BMP / TIFF redirect to lossy WebP** because the source formats either don't carry a quality dial (PNG lossless, BMP uncompressed) or don't expose one through `image` 0.25 (TIFF's LZW / Deflate / PackBits aren't reachable from the `image` encoder). Redirecting to a lossy target means the slider has the same meaning everywhere. **A reactive amber banner in staging surfaces the redirect** with per-format counts so the conversion isn't hidden ("3 files will be saved as WebP — PNG (1), BMP (1), TIF (1)"). The header copy isn't load-bearing alone — the smoke session caught that the banner was needed to make the conversion noticeable. Considered + deferred: PNG-via-imagequant + TIFF-in-format lanes (see [BACKLOG](plans/BACKLOG.md) → Image). Rejected: JPEG fallback for lossless inputs — silently flattens alpha onto white, wrong default for a "compress this image" verb.
- **Slider default `0.8`, not `0.5` (Video Compress's midpoint).** JPEG / WebP at q=50 is visibly worse than CRF-50 on video. `0.8` is the "save for web" sweet spot — visibly smaller than the source for most JPEGs while still indistinguishable at typical viewing sizes. Single linear map (`q = round(1 + 99 * slider)`) covers both encoders.
- **EXIF is stripped by side effect of the `image` decode → re-encode round-trip.** Same behaviour as Crop / Resize — pinned here so a future "preserve EXIF" instinct gets a written counter-argument. Orientation is honoured on the way in (via [`decode_oriented`](../src-tauri/multitool-core/src/image/mod.rs)) so a rotated source comes out upright; the actual EXIF block is dropped.
- **Cancellation is between files only.** A single JPEG / WebP encode is fast enough that mid-file cancel isn't useful and would complicate cleanup. Same shape as the Image Format Converter; different from Video Compress, which can kill an ffmpeg child mid-encode.
- **The staging UI's predicted-output filename mirrors the Rust strategy table in [`ImageCompress.tsx`](../src/tools/image-compress/ImageCompress.tsx).** Keep `predictedOutputExtension()` aligned with `strategy_for` in [`convert.rs`](../src-tauri/multitool-core/src/tools/image_compress/convert.rs) — drift would show up as a "predicted ≠ written" mismatch in the done view.
- **`file_size` IPC command** rather than the `tauri-plugin-fs` dep — single stat lookup doesn't justify a new plugin surface. Lives in [`src-tauri/src/system.rs`](../src-tauri/src/system.rs) alongside `supported_raster_formats`; failure here only blanks the staging-row size display, never blocks the run.

---

## Image Compress: libwebp via `webp` crate

The Image Compress tool needs **lossy** WebP encoding, which `image` 0.25 doesn't provide (see "WebP output is lossless only" below — that entry explicitly told us to weigh the dep in a new entry before adding it; this is that entry). Picked [`webp`](https://crates.io/crates/webp) 0.3 — a thin wrapper over `libwebp-sys`, which builds libwebp from source via `cc-rs`. **No autotools, no system libs** — unlike `mp3lame-sys`, which needed a `brew install autoconf automake libtool` step on the macOS CI runner. License: BSD; bundle-compatible alongside the existing GPL ffmpeg bundle.

Why lossy WebP is load-bearing for image-compress:
- **WebP→WebP re-encode** at the user's slider position (image-compress's v1 strategy preserves source format for JPEG and WebP).
- **PNG/BMP/TIFF→WebP** under-the-hood lane: lossless inputs need a lossy target so the slider has meaning; lossy WebP supports alpha so PNG transparency round-trips. JPEG fallback was rejected — it silently flattens alpha onto white, which is the wrong default for a "compress this image" verb.

CI cleared all three runners (Linux / macOS / Windows) on the first push — `libwebp-sys` builds cleanly via `cc-rs` everywhere with no extra system-dep step. If a future bump of `webp` / `libwebp-sys` regresses that on any runner, add the step to both [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) and [`.github/workflows/release.yml`](../.github/workflows/release.yml), same as the mp3lame brew step.

---

## Video Compress: single-slider quality, shared CRF range, stream-copy audio

The Video Compress tool (commits `82d388c…3249fc8`) is the third video tool. Adding it consolidated everything previously trimmer-only — preview-proxy transcode, codec-mirror tables, per-codec CRF anchors — into a shared [`multitool_core::video`](../src-tauri/multitool-core/src/video/) module + an app-level [`src-tauri/src/preview_proxy.rs`](../src-tauri/src/preview_proxy.rs). What's worth pinning:

- **Per-codec CRF lives once, in [`video_crf_range`](../src-tauri/multitool-core/src/video/reencode.rs).** Returns `(best, worst)` — `best` is the trimmer's transparent CRF (h264=18, hevc=20, vp9=28, av1=30, vp8=10), `worst` is the smallest-file-still-watchable bound (h264=32, hevc=34, vp9=40, av1=42, vp8=25). The trimmer pins `best`; the compressor lerps `worst → best` as the slider moves `0 → 1`. If midpoint compresses too aggressively/weakly on real clips, **tune the `worst` slot only** — `best` is load-bearing for the trimmer's transparent-encode contract.
- **Single slider labeled "Smaller file ←→ Higher quality", no number.** Resisted: numeric 1–10 (reads as absolute quality across codecs, which it isn't), discrete steps with names (the underlying axis is continuous CRF), live size estimate (visibly wrong on extreme content — a static screencast vs an action clip at the same slider differ by an order of magnitude). Wire shape is `Opts { quality: f64 }`; Rust does the per-codec interpolation. Out-of-range values clamp.
- **Audio is stream-copied (`-c:a copy`) in v1; `-map 0:a` with no `:0` keeps every audio track bit-perfect.** Strictly better than the trimmer's v1, which encodes all tracks with the first track's codec. Cost: the slider doesn't affect audio size, so a podcast-style clip (mostly audio at low video bitrate) sees little change. Scaling audio bitrate with the slider is the [BACKLOG](plans/BACKLOG.md) follow-up, designed as a checkbox (additive).
- **VP8 keeps a fixed `-b:v 1M` cap regardless of CRF in [`video_encoder_flags`](../src-tauri/multitool-core/src/video/reencode.rs).** VP8 needs both flags for stable quality; compress only sweeps the CRF (10–25). If VP8 sees real use through compress and the slider feels weak, add a VP8-specific scaling builder rather than overloading the shared one.
- **No `-avoid_negative_ts make_zero`** (unlike the trimmer). Compress doesn't seek into the middle of the source. Don't copy this flag over cargo-cult-style if the compress arg vec ever gets extended.
- **TS preview-proxy + duration-probe wrappers stay in [`src/lib/tools/videoTrimmer.ts`](../src/lib/tools/videoTrimmer.ts); compress imports from there.** The Rust commands are app-level since the hoist, and re-typing the four TS wrappers in `videoCompress.ts` doesn't buy anything. This is the one place the per-tool isolation bends in this tool. **If a third video tool needs them, hoist all four to `src/lib/videoPreview.ts`** (parallel to `audioPreview.ts`); the cross-tool import comment in [`VideoCompress.tsx`](../src/tools/video-compress/VideoCompress.tsx) marks this contract.
- **Slider default: `0.5` (midpoint).** Approved during smoke; tune in code if usage shows another default is better than recompressing once.

---

## Lorem Ipsum: text-only tools skip the multitool-core / IPC split

The Lorem Ipsum generator (commits `9465661…366abc5`) is the first **text** category tool and the first to ship without a Rust side. The shape worth pinning so the next text tool (e.g. diff) follows it rather than re-litigating:

- **No `multitool-core/tools/*/` folder, no `#[tauri::command]` shim, no `src/lib/tools/*.ts` wrapper.** Generation is in-component ([`src/tools/lorem/generator.ts`](../src/tools/lorem/generator.ts)). The justifications for the standard split — heavy work, file I/O, cancellation, progress — don't apply to instant pure-string work. Forcing the split would mean an IPC hop, an alias entry in [`vite.config.ts`](../vite.config.ts), and a mock under `tests/e2e/mocks/` for a function that does `Array.from({ length }, …).join(…)`. ARCHITECTURE §3.2's "webview is rendering-only" is the rule for *heavy work*; rendering text is rendering.
- **Clipboard via `navigator.clipboard.writeText`, not `tauri-plugin-clipboard-manager`.** Works in the Tauri webview without adding a plugin or a capability grant. If a future tool needs richer clipboard ops (images, MIME types, read-from-clipboard), revisit then.
- **New category = the narrow shared edit ADDING_A_TOOL allows.** Extending `ToolCategory` + `toolCategories` in [`src/tools/registry.ts`](../src/tools/registry.ts) + adding the section assertion in [`Dashboard.test.tsx`](../src/app/Dashboard.test.tsx) is the right surface for a brand-new category; it's not a registry-pattern violation.

---

## Video Trimmer: frame-accurate codec-matched re-encode, always-on WebM preview proxy via blob URL

The Video Trimmer (commits `b6c4422…`; engine re-cut to frame-accurate at `b3c6442…`) is the second video tool. It trims by a codec-matched **full re-encode**, and its preview is the load-bearing part — several decisions there cost real iteration and shouldn't be unwound:

- **Trim is a codec-matched, frame-accurate full re-encode** ([`convert.rs`](../src-tauri/multitool-core/src/tools/video_trimmer/convert.rs)). `-i <input>` then `-ss <start>` (output seek, **frame-accurate** by construction — decode from the previous keyframe, discard until the requested ms), `-t <dur>`, `-map 0:v:0 -map 0:a?`, `-avoid_negative_ts make_zero`. Encoder + flags come from the shared codec-mirror tables ([`video_encoder_flags`](../src-tauri/multitool-core/src/video/reencode.rs) / [`audio_encoder_flags`](../src-tauri/multitool-core/src/video/reencode.rs)) keyed on the source codec parsed via [`ffmpeg::probe_video_stream_params`](../src-tauri/multitool-core/src/ffmpeg.rs) / [`probe_audio_stream_params`](../src-tauri/multitool-core/src/ffmpeg.rs); the trimmer pins the transparent CRF from [`video_crf_range`](../src-tauri/multitool-core/src/video/reencode.rs): h264→libx264 CRF 18, hevc→libx265 CRF 20, vp9→libvpx-vp9 CRF 28, av1→libaom-av1 CRF 30 (bundled ffmpeg ships libaom, not libsvtav1), vp8→libvpx CRF 10, fallback libx264. Audio: aac→aac 128k, opus→libopus 128k, mp3→libmp3lame V2, flac→flac (lossless), vorbis→libvorbis q5, ac3→ac3 192k, fallback aac 128k. `-pix_fmt` is **mirrored** from the probed source so 10-bit / HDR-adjacent inputs don't silently downgrade to yuv420p — the fallback to yuv420p only fires when the probe couldn't read pixfmt. Source bitrate is *not* mirrored: CRF lets the codec spend bits where they matter (a 5980 kbps OBS static-screen source can land at ~30 kbps without visible loss), and bitrate-matching tries to bloat trivial encodes. Subtitle / data / attachment streams are dropped (A/V only). Output `{stem}_trimmed.{ext}` via `unique_path` next to the source.
- **Why not "smart cut" (partial re-encode of head/tail GOPs + middle stream-copy + concat).** It scores better on paper for quality + file size on long trims, but the implementation gap is real: head/tail segments need SPS/PPS exactly matching the source for the concat-demuxer splice to take `-c copy`, the splice is finicky across containers (mp4/mkv/mov differ), and the failure modes are subtle (audible glitches, A/V drift, codec rejection on playback). Speed doesn't matter for a learning-project trimmer and inputs are short, so a full re-encode at a perceptually-transparent CRF wins on implementation simplicity at a small quality cost. Revisit only if real usage shows the generation loss is unacceptable.
- **AAC default 128k, not 192k.** Smoke #1 caught a 4× audio-bytes over-encode against a 44 kbps smartphone clip. 128k AAC is perceptually transparent for stereo music and well above speech requirements; further compression risks degrading low-bitrate phone-capture sources less than 192k did. AC3 stays at 192k (it's typically a surround/5.1 format where 192k is already on the low side; cutting further would degrade the mix). Source-bitrate mirroring was rejected because banners frequently omit it for opus / flac / vorbis and a per-codec default has fewer edge cases.
- **Known v1 limitations (all in [BACKLOG.md](plans/BACKLOG.md) as follow-ups):** HDR sources round-trip as SDR-tagged (`-color_*` not mirrored); only `0:v:0` is mapped (rare multi-video-stream sources lose the extras); multi-audio sources keep all tracks but encode them all with the first track's codec.
- **Preview always goes through a transcoded WebM proxy, played from a blob URL — never the source's asset URL.** Two hard constraints, learned by smoke-testing, drive this:
  1. **WebKitGTK's `<video>` can't load Tauri's asset-protocol scheme at all** (its GStreamer pipeline rejects the custom scheme → `play()` throws `NotSupportedError`), regardless of codec. But `fetch()` on the asset URL works (same path the Audio Trimmer relies on). So the UI `fetch`es the proxy bytes and plays them from `URL.createObjectURL(blob)`.
  2. **WebKitGTK decodes H.264/AAC only with proprietary gstreamer plugins** that often aren't installed. So the proxy is **VP9/Opus in WebM** (`-deadline realtime -cpu-used 8 -crf 32`, scaled ≤1280w) — open codecs it ships with. An mp4/H.264 proxy fails exactly where the fallback is needed.

  Net: every pick transcodes a small WebM proxy ([`proxy.rs`](../src-tauri/multitool-core/src/video/proxy.rs)) to the OS temp dir, the frontend fetches+blobs it, and the trim still runs against the **original** file. Don't "optimize" this back to a native `<video src={assetUrl}>` — it'll work on Chromium/WebView2 and silently break on Linux.
- **Preview proxies are swept on tool mount, not just per-pick.** A hard app-close tears down the WebView without running React unmount cleanup, orphaning the in-flight proxy. `cleanup_stale_proxies` deletes every `multitool-preview-*.webm` in the temp dir (all throwaway) and runs when the tool mounts; per-pick/reset teardown handles the in-session case. Both `cleanup_*` commands are guarded to the temp dir + the `multitool-preview-` prefix so a stray IPC call can't unlink arbitrary files.
- **Duration comes from a backend probe, not `<video>.duration`.** `probe_video_duration` (wrapping `ffmpeg::probe_duration_secs`) sizes the trim window up front — reliable regardless of what the WebView can decode.
- **Player playback is clamped to the trim window.** No native `controls` (fullscreen/speed/download leak inconsistently across WebViews); a minimal bar (play/pause + seek + volume) plays only `[start, end]`. Time helpers `formatMs`/`parseMs` + the `TimeInput` component were hoisted to [`src/lib/time.ts`](../src/lib/time.ts) + [`src/components/TimeInput.tsx`](../src/components/TimeInput.tsx) (shared with the Audio Trimmer).

---

## Image Resize: lock orthogonal to fit-mode, aspect compared via integer cross-multiplication

The Image Resize tool (commits `fb57122…df31717`) is the third "preserve the source format" image tool, after Crop. Two non-obvious choices worth pinning:

- **The lock checkbox controls input slaving, not fit-mode visibility.** A natural-feeling first cut was "lock ON → output is a clean proportional scale, so hide Fit/Background; lock OFF → reveal them". A smoke session caught the failure mode: lock OFF, user types 100×100 from a 100×50 source, then re-locks → captured ratio is 1:1 ≠ source's 2:1, and any further scale stays aspect-mismatched even with the lock ON. The decisive question is whether *target aspect equals source aspect*, regardless of how the user got there. Fit and Background gate on `aspectMismatch`; the lock is purely a UI affordance for how W and H move together.
- **Aspect comparison is integer cross-multiplication, not float division.** `targetW × sourceH === sourceW × targetH` reaches a clean equality on cases like `100/50 == 200/100` where f64 division would false-positive only because the source happened to already be representable; with `MAX_DIM = 16384`, both products top out at ≈ 2.7×10⁸, well inside the 2⁵³ safe-integer range of JS numbers.
- **`JPEG_REENCODE_QUALITY` (90) and `tiff_frame_count` are shared, not per-tool.** Both encode contract-level invariants of "preserve the source format" — same quality choice for any tool that re-encodes JPEG geometrically; same up-front rejection for multi-frame TIFF before decode silently drops frames. Lifted from `image_crop::convert` to [`multitool_core::image`](../src-tauri/multitool-core/src/image/mod.rs) + [`multitool_core::image::tiff`](../src-tauri/multitool-core/src/image/tiff.rs) so the resize tool consumed them without duplication. Distinct from the format converter's *user-controlled* `jpeg_quality` knob — geometry tools intentionally hide quality choice.
- **Contain background is gated on the source format's alpha capability.** `Transparent` is rejected up-front when `fit=Contain` and the target (source) extension doesn't support alpha (JPEG / BMP); Stretch / Cover never read `background` so a stale `Transparent` choice in those modes is silently ignored, matching the front-end's hide-background-outside-Contain rule. Same gating shape as the converter's `AlphaHandling::Preserve`.
- **Cover is fixed center-crop, no anchor knob.** Matches Photoshop's "Fill" and CSS `object-fit: cover` defaults (minus `object-position`). A focal-point UI would be useful for portrait crops, but it's a separate UX scope and `image_crop` already covers explicit-region cropping. Revisit if smoke ever calls for it.

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

## WebP output is lossless only (no `webp_quality` option) — image-format-converter scope

`image` 0.25's WebP encoder is lossless-only. `TargetFormat::Webp` in image-format-converter emits lossless WebP unconditionally — no `webp_quality` field in `Opts`. A lossy lane would mean the `webp` crate (libwebp C bindings + native build dep on every CI runner) or `image-webp` (lossless-only encoder at last check). **Don't add a silent quality field that gets ignored**.

Note: the libwebp dep was eventually added — but for image-compress, which needs lossy WebP for its quality slider (see "Image Compress: libwebp via `webp` crate" above). The format-converter's lossless contract still stands; converting to lossy WebP there would be a separate decision (the converter's "no quality knob" ethos doesn't fit a quality field).

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
