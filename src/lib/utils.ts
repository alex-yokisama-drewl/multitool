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
