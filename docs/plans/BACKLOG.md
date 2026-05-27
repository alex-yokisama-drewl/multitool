# Backlog

Plans and ideas not yet committed to a milestone. When an item moves into active build, it gets its own ephemeral working doc at `docs/plans/<TOOL_NAME>.md` (see [../ADDING_A_TOOL.md](../ADDING_A_TOOL.md)) and is removed from this list.

## UX

- **Drag-and-drop input on dashboard/tools.** Drop one or more files anywhere on the dashboard or a tool screen to start a flow. Was a stretch goal for 0.2.0; still unshipped.
- **Paste-from-clipboard for image inputs.** Any tool that accepts images should allow pasting (Ctrl/Cmd+V) directly — screenshots in particular. Likely a shared input affordance rather than per-tool.
- **Video thumbnails in the staging area.** Show a first-frame thumbnail next to each picked video so the user can confirm the file before converting. Two viable paths: (a) `<video src={convertFileSrc(path)}>` with asset-protocol scope extended for picked video paths — fast to build but only renders codecs the WebView decodes natively (mp4/webm yes, mkv/avi no — would need a placeholder); (b) a new `extract_thumbnail` Tauri command that runs ffmpeg to grab a JPEG of the first frame and returns bytes via IPC — works for every codec ffmpeg can read, more plumbing. Deferred from the Video Format Converter v1 build.

## Future tools

### Image

- **HEIC support for the Image Format Converter.** Researched 2026-05-25; deferred. The `image` crate has no HEIC support. Standard Rust path is [`libheif-rs`](https://crates.io/crates/libheif-rs) / [`libheif-sys`](https://github.com/Cykooz/libheif-sys), which wrap [libheif](https://github.com/strukturag/libheif) (C). The pdfium "download a prebuilt tarball in build.rs" pattern doesn't transfer: libheif itself is just a container parser — HEIC decoding needs an HEVC backend ([libde265](https://github.com/strukturag/libde265)), and there's no canonical all-in-one prebuilt distribution. `libheif-sys`'s `embedded-libheif` feature vendors libheif's source and builds it via CMake, but does NOT bundle codec deps; libde265 must still be installed at build time. The closest precedent for a single-blob libheif is [pphh77/libheif-Windowsbinary](https://github.com/pphh77/libheif-Windowsbinary/releases) — Windows-only, single maintainer. Realistic options when picking this back up:
  - Use `libheif-sys` with `embedded-libheif`; require `libheif`/`libde265` system installs on dev + CI (apt / brew / `cargo vcpkg build`); later stage the per-OS shared libs as Tauri resources for end-user bundles.
  - Build libheif + libde265 from source in `build.rs` end-to-end (heavy: minutes-long first build, cmake + C++ compiler required on every builder).
  - Wait for the ecosystem (pure-Rust HEVC decoder, or an "all-in-one" prebuilt libheif). HEVC patents make a pure-Rust decoder unlikely soon.
  HEIC **encode** is doubly out of scope: needs x265 (GPL + patent-encumbered).
- **Image resize.**
- **Image compress.**

### Video

- **Video compress.**
- **Frame-accurate trim (full re-encode).** The Video Trimmer ships keyframe-snapped stream-copy cuts (lossless, instant) — the start snaps back to the keyframe at or before the selected point, so the real cut can land up to one full GOP early (measured: ~1–5s for typical sources, up to ~8–10s for long-GOP encodes like OBS captures; the excess is extra leading footage, since the end is frame-accurate and the selected duration isn't preserved). The fix is a **full re-encode** (decode → cut at the exact frame → re-encode): frame-accurate seeking is free once you're re-encoding, no smart-cut splice needed. This is **always on, not a setting** — accuracy beats speed here. The re-encode should aim to mirror the source's params (codec / bitrate / pixfmt) to minimize the quality generation. Shares the re-encode backend with **Video compress** via a helper in `multitool-core` (a shared core module both tools call — not one tool depending on the other).

### Text

- **Lorem ipsum generator.** Generate placeholder text. No configuration controls — just two buttons: copy and re-generate.
- **Text diff.**
