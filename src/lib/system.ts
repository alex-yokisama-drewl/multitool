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

export async function pickPdfFile(): Promise<string | null> {
  const result = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  return typeof result === "string" ? result : null;
}

// Multi-select picker for the Images → PDF tool. Returns the picked paths
// in the order the OS dialog produced them (the tool sorts them ascending
// before staging). Tauri's plugin-dialog returns `null` on cancel and an
// array of one-or-more paths on confirm — never an empty array — so this
// preserves that null-vs-array contract for callers.
export async function pickImageFiles(): Promise<string[] | null> {
  const result = await open({
    multiple: true,
    directory: false,
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  if (result === null) return null;
  return Array.isArray(result) ? result : [result];
}

// Multi-select picker for the Image Format Converter tool. Accepts every
// raster format the converter decodes, plus SVG. The filter is advisory —
// the Rust side re-validates by attempting a decode, so a renamed file
// still routes through the skip+continue path rather than crashing.
export async function pickConvertibleImages(): Promise<string[] | null> {
  const result = await open({
    multiple: true,
    directory: false,
    filters: [
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
    ],
  });
  if (result === null) return null;
  return Array.isArray(result) ? result : [result];
}

// Single-select picker for the Image Crop tool. The crop output preserves
// the source format, so the dialog filter is restricted to the encodable
// raster set — fetched from the backend via `rasterImageExtensions()` so it
// can't drift from the Rust `RasterFormat` list. The filter is advisory; the
// Rust side re-validates on decode + extension match.
export async function pickRasterImage(): Promise<string | null> {
  const extensions = await rasterImageExtensions();
  const result = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "Images", extensions }],
  });
  return typeof result === "string" ? result : null;
}

// Multi-select picker for the Audio Format Converter tool. Accepts every
// audio format the Rust side decodes via Symphonia (default features + the
// `mp3`/`aac`/`alac`/`isomp4`/`aiff`/`caf` feature flags enabled in
// `multitool-core`). The filter is advisory — Symphonia re-validates on
// decode, so a renamed file routes through skip+continue.
export async function pickConvertibleAudio(): Promise<string[] | null> {
  const result = await open({
    multiple: true,
    directory: false,
    filters: [
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
    ],
  });
  if (result === null) return null;
  return Array.isArray(result) ? result : [result];
}

// Single-select picker for the Audio Trimmer tool. Restricted to the
// formats we can round-trip back to the source (the trimmer's contract
// is "output preserves source format"; we have encoders only for
// wav/mp3/flac/ogg/oga). The filter is advisory; the Rust side
// re-validates via decode + extension match.
export async function pickAudioFile(): Promise<string | null> {
  const result = await open({
    multiple: false,
    directory: false,
    filters: [
      {
        name: "Audio",
        extensions: ["mp3", "wav", "flac", "ogg", "oga"],
      },
    ],
  });
  return typeof result === "string" ? result : null;
}

// Single-select picker for the Audio Extractor tool. Same extension
// filter as `pickVideoFiles` — the filter is advisory, ffmpeg sniffs the
// actual container at decode time. Returns `null` on cancel.
export async function pickVideoFile(): Promise<string | null> {
  const result = await open({
    multiple: false,
    directory: false,
    filters: [
      {
        name: "Video",
        extensions: [
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
        ],
      },
    ],
  });
  return typeof result === "string" ? result : null;
}

// Multi-select picker for the Video Format Converter tool. The filter
// is advisory — ffmpeg sniffs the actual container at decode time, so a
// renamed file still routes through the orchestrator's skip+continue.
// List covers the common video containers ffmpeg's standard GPL build
// can demux; an exotic extension can still be picked via the dialog's
// "All files" fallback if the user knows ffmpeg supports it.
export async function pickVideoFiles(): Promise<string[] | null> {
  const result = await open({
    multiple: true,
    directory: false,
    filters: [
      {
        name: "Video",
        extensions: [
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
        ],
      },
    ],
  });
  if (result === null) return null;
  return Array.isArray(result) ? result : [result];
}

export async function revealInFolder(path: string): Promise<void> {
  await revealItemInDir(path);
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
