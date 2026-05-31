// IPC wrapper for the Video Compress tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// here so components stay presentational and Playwright can mock at the
// `src/lib/` seam (ARCHITECTURE.md §3.2). The preview-proxy / duration
// probe live in `videoTrimmer.ts` — they're shared with the trimmer at
// the Rust side (see `src-tauri/src/preview_proxy.rs`) and re-imported
// from there in this tool's component so we don't duplicate wrapper code.

import { runJob, type JobHooks } from "../jobRunner";
import type { AppErrorEnvelope } from "../errors";

// Mirrors `multitool_core::tools::video_compress::Opts`. `quality` in
// `[0.0, 1.0]`: 0 = smallest file, 1 = highest quality (matches the
// trimmer's transparent CRF for the source codec). Out-of-range values
// clamp silently on the Rust side.
export interface Opts {
  quality: number;
}

// Mirrors `multitool_core::tools::video_compress::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]`. `started` once,
// then `file-progress` with the mid-encode 0..=1 fraction.
export type Progress =
  | { kind: "started"; source: string }
  | { kind: "file-progress"; source: string; fraction: number };

// Mirrors `JobResult`. `output` is the FILE path of the compressed
// clip — pass straight to `revealInFolder`.
export interface JobResult {
  output: string;
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type CompressHooks = JobHooks<Progress>;

export async function compressVideo(
  path: string,
  opts: Opts,
  hooks: CompressHooks = {},
): Promise<JobResult> {
  return runJob<{ path: string; opts: Opts }, Progress, JobResult>(
    "compress_video",
    { path, opts },
    hooks,
  );
}

// Re-export so callers don't reach back into `../errors` directly.
export type VideoCompressError = AppErrorEnvelope;
