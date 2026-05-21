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

export async function revealInFolder(path: string): Promise<void> {
  await revealItemInDir(path);
}
