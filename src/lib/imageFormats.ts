// Single source of truth for the raster formats the pipeline can both decode
// and encode. Served by the `supported_raster_formats` Tauri command (backed
// by `multitool_core::image::RasterFormat`) so the picker filters and the
// format-converter dropdown never duplicate the Rust list.
//
// Boundary file: the only `@tauri-apps/api` call lives here so components stay
// presentational and Playwright mocks at the `src/lib/` seam (ARCHITECTURE §6).

import { invoke } from "@tauri-apps/api/core";

// Mirrors `multitool_core::image::RasterFormatDescriptor` (serde snake_case).
export interface RasterFormatDescriptor {
  id: string;
  name: string;
  extensions: string[];
  supports_alpha: boolean;
}

// Session-scoped memo: the format set is compile-time-constant on the Rust
// side, so one fetch per app session is enough. We cache the in-flight
// promise (deduping concurrent callers) but clear it on rejection so a
// transient failure doesn't poison every later read.
let cache: Promise<RasterFormatDescriptor[]> | null = null;

export async function getRasterFormats(): Promise<RasterFormatDescriptor[]> {
  cache ??= invoke<RasterFormatDescriptor[]>("supported_raster_formats").catch(
    (err: unknown) => {
      cache = null;
      throw err;
    },
  );
  return cache;
}

// Flattened list of accepted extensions (e.g. ["png","jpg","jpeg",…]) for an
// OS picker's dialog filter.
export async function rasterImageExtensions(): Promise<string[]> {
  const formats = await getRasterFormats();
  return formats.flatMap((f) => f.extensions);
}

// Test-only: drop the session memo so each case starts from a cold fetch.
// Exported (not just internal) because the memo is module-scoped and Vitest
// shares module state across cases in a file.
export function __resetRasterFormatsCache(): void {
  cache = null;
}
