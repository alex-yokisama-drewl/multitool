// IPC wrapper for the Image Compress tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §3.2).

import { runJob, type JobHooks } from "../jobRunner";
import type { AppErrorEnvelope } from "../errors";

// Mirrors `multitool_core::tools::image_compress::Opts`. `quality` in
// `[0.0, 1.0]`: 0 = smallest file, 1 = highest quality. Same wire shape
// as `videoCompress`; out-of-range values clamp silently on the Rust
// side.
export interface Opts {
  quality: number;
}

// Mirrors `multitool_core::tools::image_compress::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]`. Each file fires
// `started` first, then either `succeeded` (with the before/after byte
// counts for the size delta) or `skipped`.
export type Progress =
  | { kind: "started"; index: number; total: number; source: string }
  | {
      kind: "succeeded";
      index: number;
      total: number;
      source: string;
      output: string;
      original_bytes: number;
      compressed_bytes: number;
    }
  | {
      kind: "skipped";
      index: number;
      total: number;
      source: string;
      error: AppErrorEnvelope;
    };

// One entry per successfully-compressed file. Mirrors
// `multitool_core::tools::image_compress::CompressedFile`.
export interface CompressedFile {
  source: string;
  output: string;
  original_bytes: number;
  compressed_bytes: number;
}

export interface SkippedFile {
  source: string;
  error: AppErrorEnvelope;
}

// Mirrors `JobResult`. `first_output_path` is the FILE path of the first
// successful output — pass it straight to `revealItemInDir`. `null` when
// no file succeeded.
export interface JobResult {
  success_count: number;
  skip_count: number;
  outputs: CompressedFile[];
  skipped: SkippedFile[];
  first_output_path: string | null;
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type CompressHooks = JobHooks<Progress>;

export async function compressImages(
  paths: string[],
  opts: Opts,
  hooks: CompressHooks = {},
): Promise<JobResult> {
  return runJob<{ paths: string[]; opts: Opts }, Progress, JobResult>(
    "compress_images",
    { paths, opts },
    hooks,
  );
}
