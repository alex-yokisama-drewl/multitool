// IPC wrapper for the Image Format Converter tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).

import { runJob, type JobHooks } from "../jobRunner";
import type { AppErrorEnvelope } from "../errors";

// Mirrors `multitool_core::tools::image_format_converter::TargetFormat`
// with `#[serde(rename_all = "lowercase")]`. Keep the variants in sync —
// the Rust side gates encoder selection on these strings.
export type TargetFormat = "png" | "jpeg" | "webp" | "bmp" | "tiff";

// Mirrors `AlphaHandling` (`#[serde(rename_all = "kebab-case")]`).
export type AlphaHandling = "preserve" | "flatten-white" | "flatten-black";

// Mirrors `SvgRasterSize` (`#[serde(rename_all = "kebab-case")]`).
// `Natural` → "natural" (unit variant); `LongestEdgePx(n)` →
// { "longest-edge-px": n } (newtype variant in serde's externally-tagged
// representation).
export type SvgRasterSize = "natural" | { "longest-edge-px": number };

export interface Opts {
  target_format: TargetFormat;
  jpeg_quality: number;
  alpha_handling: AlphaHandling;
  svg_raster_size: SvgRasterSize;
}

// Mirrors `multitool_core::tools::image_format_converter::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]` flattens each
// variant's fields next to a `kind` discriminator.
export type Progress =
  | {
      kind: "started";
      index: number;
      total: number;
      source: string;
    }
  | {
      kind: "succeeded";
      index: number;
      total: number;
      source: string;
      output: string;
      warnings: string[];
    }
  | {
      kind: "skipped";
      index: number;
      total: number;
      source: string;
      error: AppErrorEnvelope;
    };

export interface SkippedFile {
  source: string;
  error: AppErrorEnvelope;
}

// Mirrors `JobResult`. `first_output_path` is the FILE path of the first
// successful output — pass it straight to `revealItemInDir` (which opens
// the parent folder with the file highlighted). `null` when no file
// succeeded.
export interface JobResult {
  success_count: number;
  skip_count: number;
  skipped: SkippedFile[];
  first_output_path: string | null;
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type ConvertHooks = JobHooks<Progress>;

export async function convertImageFormat(
  paths: string[],
  opts: Opts,
  hooks: ConvertHooks = {},
): Promise<JobResult> {
  return runJob<{ paths: string[]; opts: Opts }, Progress, JobResult>(
    "convert_image_format",
    { paths, opts },
    hooks,
  );
}
