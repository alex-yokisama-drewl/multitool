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

// Wrap convertFileSrc so all `@tauri-apps/api` access happens at the
// system.ts seam — same pattern as the picker / opener wrappers. The
// Playwright mock under `tests/e2e/mocks/system.ts` returns a placeholder
// URL since Tauri's `__TAURI_INTERNALS__` global isn't available in a
// regular Chromium.
export function imageAssetUrl(path: string): string {
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

export async function revealInFolder(path: string): Promise<void> {
  await revealItemInDir(path);
}

// Grant the webview asset-protocol access to a set of picked image paths,
// so `convertFileSrc(path)` can render thumbnails. Tauri's asset-protocol
// scope starts empty by default (see DECISIONS.md → "Asset protocol scope:
// dynamic per-pick"); this is the per-pick widening. The Rust side
// re-validates `.png/.jpg/.jpeg/.webp` so a direct IPC call can't widen
// the grant past image files.
export async function allowImagePreview(paths: string[]): Promise<void> {
  await invoke("allow_image_preview", { paths });
}
