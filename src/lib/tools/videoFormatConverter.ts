// IPC wrapper for the Video Format Converter tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).

import { runJob, type JobHooks } from "../jobRunner";
import type { AppErrorEnvelope } from "../errors";

// Mirrors `multitool_core::tools::video_format_converter::TargetFormat`
// with `#[serde(rename_all = "lowercase")]`. Keep variants in sync — the
// Rust side gates codec selection on these strings.
export type TargetFormat = "mp4" | "webm" | "mkv";

export interface Opts {
  target_format: TargetFormat;
}

// Mirrors `multitool_core::tools::video_format_converter::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]` flattens each
// variant's fields next to a `kind` discriminator. `file-progress`
// streams mid-encode 0..=1 fractions for the file currently in-flight;
// the shim throttles to ~4 events/sec.
export type Progress =
  | {
      kind: "started";
      index: number;
      total: number;
      source: string;
    }
  | {
      kind: "file-progress";
      index: number;
      total: number;
      source: string;
      fraction: number;
    }
  | {
      kind: "succeeded";
      index: number;
      total: number;
      source: string;
      output: string;
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
// successful output — pass it straight to `revealInFolder`. `null` when
// no file succeeded.
export interface JobResult {
  success_count: number;
  skip_count: number;
  skipped: SkippedFile[];
  first_output_path: string | null;
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type ConvertHooks = JobHooks<Progress>;

export async function convertVideoFormat(
  paths: string[],
  opts: Opts,
  hooks: ConvertHooks = {},
): Promise<JobResult> {
  return runJob<{ paths: string[]; opts: Opts }, Progress, JobResult>(
    "convert_video_format",
    { paths, opts },
    hooks,
  );
}
