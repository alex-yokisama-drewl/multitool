# Feature: Settings memory (directory + tool options)

Ephemeral working doc. Delete when the feature ships. Flip rows `pending → done`
with the SHA + a gotcha note after every commit (CLAUDE.md → "Keep the plan live").

## Summary

Persist two kinds of state across app restarts: the last directory used per
picker media kind, and the per-tool option values that express how the user
likes to work.

This is **not** a tool, so it doesn't follow ADDING_A_TOOL. It is a shared
surface, which per CLAUDE.md means it needs a DECISIONS entry explaining why
shared files changed.

## Decisions taken up front

Approved by @alex-yokisama-drewl before the build started.

### Storage: hand-rolled JSON command, no new plugin

Two Tauri commands (`load_settings` / `save_settings`) over a versioned JSON
file in the app-config dir. Pure serde + atomic-write logic lives in
`multitool-core/src/settings.rs` so it is unit-testable without Tauri; the
shell resolves the path and owns the commands.

Rejected `tauri-plugin-store` (new dep + capability grant for a generic
key-value API wider than we need) and webview `localStorage` (not an
inspectable config file, wiped with webview data, puts durable state outside
the Rust boundary). Follows the `file_size` precedent — a narrow command
rather than `tauri-plugin-fs`.

Rust stores an **opaque JSON blob keyed by string**. The frontend owns every
schema. This keeps the registry contract intact: adding a tool never touches
Rust settings code.

### Directory memory keyed by picker media kind

Not per-tool, not one global directory. The key is what the picker's extension
filter shows, because that is what decides whether a remembered folder is
useful — reopening the video trimmer where you last picked PNGs shows an empty
dialog, which is worse than no memory.

| Key | Pickers |
| --- | --- |
| `pdf` | `pickPdfFile` |
| `image` | `pickImageFiles`, `pickConvertibleImages`, `pickRasterImage` |
| `audio` | `pickConvertibleAudio`, `pickAudioFile` |
| `video` | `pickVideoFile`, `pickVideoFiles` |

Deliberately **not** `ToolCategory` — Images → PDF is category `pdf` but picks
images, so category would send it to the wrong folder.

Refinements: a global last-directory fallback for a kind never used before, and
an existence check before handing the path to the dialog (removable drives,
deleted folders) with fall-through when it fails.

Lands entirely inside `src/lib/system.ts`. No tool component changes; new tools
inherit it by using the existing pickers.

### Which options stick

Rule: remember a setting if it expresses *how the user works*; don't if it is
derived from *the file in front of them*.

| Tool | Remembered | Not remembered |
| --- | --- | --- |
| Image Format Converter | target format, JPEG quality, alpha handling, SVG raster size (mode + px) | — |
| Image Crop | "Lock proportions" toggle | crop rect |
| Image Resize | fit mode, background, "Lock aspect ratio" toggle | width/height (auto-filled from source dims) |
| Image Compress | quality | — |
| Images → PDF | page size | staged file list + order |
| PDF → Images | format, DPI | — |
| Audio Format Converter | target format, MP3 bitrate, Vorbis quality, WAV bit depth, channel mode | — |
| Audio Trimmer | fade-in / fade-out toggles | start/end ms |
| Video Format Converter | target format | — |
| Video Trimmer | — | start/end ms, playhead |
| Video Compress | quality | — |
| Audio Extractor | — (no options) | — |
| Lorem Ipsum | — (no options) | — |

Lock toggles persist as the **boolean**, never the numeric ratio — the ratio is
re-derived from whichever image just loaded.

Preview volume (Video Trimmer + Video Compress) persists **app-wide**, not
per-tool: it is a playback preference, not a conversion option.

### Behaviour change

`reset()` ("Convert another" / "Compress another") currently snaps options back
to hardcoded defaults. With memory on it clears only job state and leaves
options alone — otherwise the feature defeats itself on the most common path.

## Tasks

| # | Commit | Status | SHA | Notes |
| --- | --- | --- | --- | --- |
| 1 | `docs(settings): working doc + backlog entry for B&W normalizer` | pending | | |
| 2 | `feat(settings): versioned JSON settings store in multitool-core` | pending | | |
| 3 | `feat(settings): load/save Tauri commands + app-config path resolution` | pending | | |
| 4 | `feat(settings): frontend prefs store + IPC wrapper` | pending | | |
| 5 | `feat(settings): remember last directory per picker media kind` | pending | | |
| 6 | `feat(settings): persist tool options` | pending | | |
| 7 | `test(settings): e2e coverage` | pending | | |
| 8 | `docs(settings): DECISIONS entry` | pending | | |

## Acceptance

- [ ] Picking a file in any tool, quitting, relaunching → that tool's picker opens in the same directory
- [ ] An image tool and a video tool remember separate directories
- [ ] A kind never used before falls back to the last directory used anywhere
- [ ] A remembered directory that no longer exists falls through without error
- [ ] PDF → Images DPI + format survive a restart
- [ ] Crop rect and resize width/height do **not** persist
- [ ] A corrupt or hand-edited settings file loads as defaults instead of crashing
- [ ] A persisted value whose variant no longer exists sanitizes back to the default
- [ ] "Convert another" keeps the user's options
