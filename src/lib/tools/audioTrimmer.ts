// IPC wrapper for the Audio Trimmer tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).
//
// Single-file shape: the picker is single-select, so the wrapper takes a
// `path: string` rather than the `paths: string[]` the format-converter
// uses. The progress stream only has a `started` variant — there's no
// skip-and-continue path on a single file, so a per-file failure
// becomes the rejected `runJob` promise carrying an `AppErrorEnvelope`.

import { runJob, type JobHooks } from "../jobRunner";

export interface Opts {
  start_ms: number;
  end_ms: number;
  fade_in_ms: number;
  fade_out_ms: number;
}

// Mirrors `multitool_core::tools::audio_trimmer::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]`. Single variant
// today; kept as a discriminated shape so widening to additional
// variants later is purely additive.
export interface Progress {
  kind: "started";
  source: string;
}

// Mirrors `JobResult` on the Rust side. `output` is the FILE path of the
// written trim — pass straight to `revealItemInDir`.
export interface JobResult {
  output: string;
  warnings: string[];
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type TrimHooks = JobHooks<Progress>;

export async function trimAudio(
  path: string,
  opts: Opts,
  hooks: TrimHooks = {},
): Promise<JobResult> {
  return runJob<{ path: string; opts: Opts }, Progress, JobResult>(
    "trim_audio",
    { path, opts },
    hooks,
  );
}
