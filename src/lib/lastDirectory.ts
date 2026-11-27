// Remembered last-used directory, keyed by picker media kind.
//
// Pure prefs logic, deliberately split from `./system.ts` so it is testable
// without mocking the dialog plugin. `system.ts` stays the one seam where
// `@tauri-apps/*` is touched; this file decides *which* directory it should
// open in.
//
// ## Why media kind, and not per-tool or one global directory
//
// Every picker in `system.ts` is extension-filtered, and that filter decides
// whether a remembered folder is any use. Reopening the video trimmer where
// you last picked PNGs shows a dialog with nothing in it — worse than no
// memory at all. So the key is what the picker *shows*.
//
// Per-tool memory would be wrong the other way: within a media kind the
// directory is nearly always the same. People crop, resize and compress the
// same photos out of the same folder, and per-tool memory would make them
// navigate there three times for one batch of work.
//
// Note this is **not** `ToolCategory` from the registry. Images → PDF is
// category `pdf` but picks images, so category would send it to the wrong
// folder.

import { definePrefs, nonEmptyString } from "./prefs";
import { parentDir } from "./utils";

/** What a picker's extension filter shows. */
export type PickerKind = "pdf" | "image" | "audio" | "video";

/**
 * Per-kind directories plus `any`, the most recent directory picked in any
 * kind. `any` is the fallback for a kind used for the first time: when it
 * guesses wrong the user is no worse off than with no memory at all, and
 * when every asset for a job sits in one project folder it is exactly right.
 *
 * Empty string means "nothing remembered" — `nonEmptyString` rejects it on
 * read, so a blank never reaches the dialog.
 */
const directories = definePrefs(
  "app:last-directory",
  { pdf: "", image: "", audio: "", video: "", any: "" },
  {
    pdf: nonEmptyString(),
    image: nonEmptyString(),
    audio: nonEmptyString(),
    video: nonEmptyString(),
    any: nonEmptyString(),
  },
);

/**
 * Record the directory of a just-picked file against its kind.
 *
 * Takes the file path rather than a directory so callers can hand over the
 * picker's result directly. A path with no parent directory is ignored.
 */
export function rememberDirectory(kind: PickerKind, filePath: string): void {
  const dir = parentDir(filePath);
  if (dir === "") return;
  directories.write(kind, dir);
  directories.write("any", dir);
}

/**
 * Directories to try for `kind`, best first: the kind's own memory, then
 * the global fallback. Deduplicated, and empties dropped.
 *
 * Returning candidates rather than one answer keeps the existence check —
 * which needs IPC — on the `system.ts` side of the seam.
 */
export function directoryCandidates(kind: PickerKind): string[] {
  const current = directories.read();
  return [...new Set([current[kind], current.any])].filter(
    (dir) => dir.length > 0,
  );
}
