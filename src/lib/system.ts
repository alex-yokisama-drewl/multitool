// Thin wrappers around Tauri plugin-dialog / plugin-opener.
//
// Components stay presentational and Playwright mocks the OS-touching calls
// at this seam — same pattern as the per-tool IPC wrappers in `./tools/`.

import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

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
