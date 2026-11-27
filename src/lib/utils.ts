import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Extract the filename from a path-like string. Handles both forward and
 * backward slashes so picked paths from Tauri's dialog (which return the
 * OS-native separator) render the same on Linux/macOS and Windows.
 *
 * Empty input or no separator returns the input unchanged.
 */
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

/**
 * Strip the final extension from a filename / path. Multi-dot stems like
 * `archive.tar.gz` yield `archive.tar`, matching Rust's `Path::file_stem`
 * semantics — that symmetry is load-bearing for the Images → PDF output
 * preview, which mirrors the orchestrator's output naming.
 */
export function fileStem(path: string): string {
  const name = fileName(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

/**
 * Format a byte count as a short human-readable string. Uses SI-decimal
 * units (1 KB = 1000 B) to match what macOS Finder, Windows Explorer, and
 * the GNOME file manager all show, so the values line up with what users
 * see when they check a file's size outside the app. One decimal place for
 * sub-100 values, zero above that, capped at TB. Negative values pass
 * through with a leading minus.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes)) return "—";
  const sign = bytes < 0 ? "-" : "";
  let n = Math.abs(bytes);
  const units = ["B", "KB", "MB", "GB", "TB"];
  let unit = 0;
  while (n >= 1000 && unit < units.length - 1) {
    n /= 1000;
    unit += 1;
  }
  const formatted =
    unit === 0 ? n.toString() : n < 100 ? n.toFixed(1) : n.toFixed(0);
  return `${sign}${formatted} ${units[unit] ?? "B"}`;
}

/**
 * Directory portion of a path-like string. Handles both separators so a
 * picked path from Tauri's dialog behaves the same on every OS.
 *
 * Returns `""` when there is no parent to speak of (a bare filename), and
 * keeps the root separator for a top-level file (`/a.txt` → `/`). Trailing
 * separators are ignored, so a directory path returns its own parent rather
 * than itself.
 */
export function parentDir(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (cut < 0) return "";
  // Root-level file: keep the separator, since "" would mean "unknown".
  if (cut === 0) return trimmed.slice(0, 1);
  return trimmed.slice(0, cut);
}
