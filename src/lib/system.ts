// Thin wrappers around Tauri plugin-dialog / plugin-opener.
//
// Components stay presentational and Playwright mocks the OS-touching calls
// at this seam — same pattern as the per-tool IPC wrappers in `./tools/`.

import {
  convertFileSrc as rawConvertFileSrc,
  invoke,
} from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { rasterImageExtensions } from "./imageFormats";
import {
  directoryCandidates,
  rememberDirectory,
  type PickerKind,
} from "./lastDirectory";

// Wrap convertFileSrc so all `@tauri-apps/api` access happens at the
// system.ts seam — same pattern as the picker / opener wrappers. The
// Playwright mock under `tests/e2e/mocks/system.ts` returns a placeholder
// URL since Tauri's `__TAURI_INTERNALS__` global isn't available in a
// regular Chromium.
export function imageAssetUrl(path: string): string {
  return rawConvertFileSrc(path);
}

// Same wrapper, audio-shaped name — the trimmer feeds the URL into
// `fetch()` + `AudioContext.decodeAudioData`, not `<img>`. One impl;
// two clearly-named call sites so the picker→preview pipeline reads
// idiomatically on both tools.
export function audioAssetUrl(path: string): string {
  return rawConvertFileSrc(path);
}

// Same wrapper, video-shaped name — the Video Trimmer feeds the URL into a
// `<video>` element (either the picked source directly, or a transcoded
// preview proxy). One impl, a clearly-named call site.
export function videoAssetUrl(path: string): string {
  return rawConvertFileSrc(path);
}

// --- picker plumbing -------------------------------------------------------
//
// Every picker below opens in the directory the user last picked that kind
// of file from, and records where they landed. Both halves live here rather
// than in the tool components, so a new tool inherits the behaviour by
// calling an existing picker and the registry contract stays intact
// (ARCHITECTURE §3.1). The key is the picker's media kind — see
// `./lastDirectory.ts` for why it isn't per-tool or one global directory.

interface Filter {
  name: string;
  extensions: string[];
}

/**
 * First remembered directory for `kind` that still exists, or `undefined`
 * to let the OS choose.
 *
 * The existence check matters: removable drives get unplugged and folders
 * get deleted between sessions, and a dialog pointed at a vanished path
 * behaves differently on every platform.
 */
async function defaultPathFor(kind: PickerKind): Promise<string | undefined> {
  for (const dir of directoryCandidates(kind)) {
    if (await invoke<boolean>("directory_exists", { path: dir })) return dir;
  }
  return undefined;
}

async function pickOne(
  kind: PickerKind,
  filters: Filter[],
): Promise<string | null> {
  const result = await open({
    multiple: false,
    directory: false,
    filters,
    defaultPath: await defaultPathFor(kind),
  });
  const path = typeof result === "string" ? result : null;
  if (path !== null) rememberDirectory(kind, path);
  return path;
}

/**
 * Tauri's plugin-dialog returns `null` on cancel and an array of
 * one-or-more paths on confirm — never an empty array. Callers depend on
 * that null-vs-array contract, so it is preserved here.
 */
async function pickMany(
  kind: PickerKind,
  filters: Filter[],
): Promise<string[] | null> {
  const result = await open({
    multiple: true,
    directory: false,
    filters,
    defaultPath: await defaultPathFor(kind),
  });
  if (result === null) return null;
  const paths = Array.isArray(result) ? result : [result];
  // Remember the first path's directory: a multi-select dialog can only
  // span one folder, so any element answers the same question.
  if (paths[0] !== undefined) rememberDirectory(kind, paths[0]);
  return paths;
}

// Covers the common containers ffmpeg's standard GPL build can demux.
// Advisory only: an exotic extension can still be picked via the dialog's
// "All files" fallback if the user knows ffmpeg supports it.
const VIDEO_EXTENSIONS = [
  "mp4",
  "m4v",
  "mov",
  "mkv",
  "webm",
  "avi",
  "3gp",
  "3g2",
  "ts",
  "mts",
  "m2ts",
  "mxf",
  "flv",
  "ogv",
  "wmv",
  "asf",
  "vob",
  "divx",
  "mpg",
  "mpeg",
];

export async function pickPdfFile(): Promise<string | null> {
  return await pickOne("pdf", [{ name: "PDF", extensions: ["pdf"] }]);
}

// Multi-select picker for the Images → PDF tool. Returns the picked paths
// in the order the OS dialog produced them (the tool sorts them ascending
// before staging).
export async function pickImageFiles(): Promise<string[] | null> {
  return await pickMany("image", [
    { name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] },
  ]);
}

// Multi-select picker for the Image Format Converter tool. Accepts every
// raster format the converter decodes, plus SVG. The filter is advisory —
// the Rust side re-validates by attempting a decode, so a renamed file
// still routes through the skip+continue path rather than crashing.
export async function pickConvertibleImages(): Promise<string[] | null> {
  return await pickMany("image", [
    {
      name: "Images",
      extensions: [
        "png",
        "jpg",
        "jpeg",
        "webp",
        "bmp",
        "tif",
        "tiff",
        "gif",
        "ico",
        "tga",
        "pbm",
        "pgm",
        "ppm",
        "pnm",
        "qoi",
        "svg",
      ],
    },
  ]);
}

// Single-select picker for the Image Crop tool. The crop output preserves
// the source format, so the dialog filter is restricted to the encodable
// raster set — fetched from the backend via `rasterImageExtensions()` so it
// can't drift from the Rust `RasterFormat` list. The filter is advisory; the
// Rust side re-validates on decode + extension match.
export async function pickRasterImage(): Promise<string | null> {
  const extensions = await rasterImageExtensions();
  return await pickOne("image", [{ name: "Images", extensions }]);
}

// Multi-select picker for the Audio Format Converter tool. Accepts every
// audio format the Rust side decodes via Symphonia (default features + the
// `mp3`/`aac`/`alac`/`isomp4`/`aiff`/`caf` feature flags enabled in
// `multitool-core`). The filter is advisory — Symphonia re-validates on
// decode, so a renamed file routes through skip+continue.
export async function pickConvertibleAudio(): Promise<string[] | null> {
  return await pickMany("audio", [
    {
      name: "Audio",
      extensions: [
        "mp3",
        "wav",
        "flac",
        "ogg",
        "oga",
        "m4a",
        "mp4",
        "aac",
        "aiff",
        "aif",
        "caf",
        "mkv",
        "webm",
      ],
    },
  ]);
}

// Single-select picker for the Audio Trimmer tool. Restricted to the
// formats we can round-trip back to the source (the trimmer's contract
// is "output preserves source format"; we have encoders only for
// wav/mp3/flac/ogg/oga). The filter is advisory; the Rust side
// re-validates via decode + extension match.
export async function pickAudioFile(): Promise<string | null> {
  return await pickOne("audio", [
    { name: "Audio", extensions: ["mp3", "wav", "flac", "ogg", "oga"] },
  ]);
}

// Single-select picker for the Audio Extractor tool. Same extension
// filter as `pickVideoFiles` — the filter is advisory, ffmpeg sniffs the
// actual container at decode time. Returns `null` on cancel.
export async function pickVideoFile(): Promise<string | null> {
  return await pickOne("video", [
    { name: "Video", extensions: VIDEO_EXTENSIONS },
  ]);
}

// Multi-select picker for the Video Format Converter tool. The filter
// is advisory — ffmpeg sniffs the actual container at decode time, so a
// renamed file still routes through the orchestrator's skip+continue.
export async function pickVideoFiles(): Promise<string[] | null> {
  return await pickMany("video", [
    { name: "Video", extensions: VIDEO_EXTENSIONS },
  ]);
}

export async function revealInFolder(path: string): Promise<void> {
  await revealItemInDir(path);
}

// Stat a file via the backend (avoids pulling in `tauri-plugin-fs` for a
// single size lookup). Used by the Image Compress staging UI to show
// source sizes before the run starts. Throws the `AppErrorEnvelope`
// shape on missing / permission-denied paths.
export async function fileSize(path: string): Promise<number> {
  return await invoke<number>("file_size", { path });
}

// Grant the webview asset-protocol access to a set of picked media paths
// (images or audio), so `convertFileSrc(path)` can render thumbnails or
// stream into `<audio>` / `AudioContext.decodeAudioData`. Tauri's asset-
// protocol scope starts empty by default (see DECISIONS.md → "Asset
// protocol scope: dynamic per-pick"); this is the per-pick widening. The
// Rust side re-validates the extension against `IMAGE_EXTS + AUDIO_EXTS`
// so a direct IPC call can't widen the grant past supported media.
export async function allowMediaPreview(paths: string[]): Promise<void> {
  await invoke("allow_media_preview", { paths });
}
