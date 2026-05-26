// IPC wrapper for the Audio Extractor tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).
//
// Shape: 1 video input → N MP3 outputs (one per audio track in the
// source). No `Opts` — v1 has zero user-facing knobs. Single-file
// semantics: any job-level failure rejects the `runJob` promise with an
// `AppErrorEnvelope` (no Skipped track variant).

import { runJob, type JobHooks } from "../jobRunner";

// Mirrors `multitool_core::tools::audio_extractor::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]` flattens each
// variant's fields next to a `kind` discriminator. `file-progress`
// streams mid-encode 0..=1 fractions for the track currently in-flight;
// the shim throttles to ~4 events/sec.
export type Progress =
  | {
      kind: "started";
      index: number;
      total: number;
    }
  | {
      kind: "file-progress";
      index: number;
      total: number;
      fraction: number;
    }
  | {
      kind: "succeeded";
      index: number;
      total: number;
      output: string;
    };

// Mirrors `JobResult`. `outputs` is in track order — `outputs[0]` is the
// natural "Open output folder" target on the UI side. `track_count` is
// always > 0 on a successful run (a source with zero audio streams is
// rejected by the orchestrator with `ProcessingFailed`).
export interface JobResult {
  track_count: number;
  outputs: string[];
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type ExtractHooks = JobHooks<Progress>;

export async function extractAudio(
  path: string,
  hooks: ExtractHooks = {},
): Promise<JobResult> {
  return runJob<{ path: string }, Progress, JobResult>(
    "extract_audio",
    { path },
    hooks,
  );
}
