// IPC wrapper for the PDF → Images tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).
// The wrapper itself is intentionally thin — its only job is to name the
// Rust command and shape the args; mechanics (jobId, progress filter,
// abort wiring, unlisten) live in `runJob`.

import { runJob, type JobHooks } from "../jobRunner";

export type Format = "png" | "jpeg";

export interface Opts {
  format: Format;
  dpi: number;
}

// Mirrors `multitool_core::tools::pdf_to_images::JobResult`. Field names
// match the Rust serde output (snake_case) so this stays a thin shape
// adapter — no field renames to keep out of sync.
export interface JobResult {
  output_dir: string;
  page_count: number;
  duration_ms: number;
}

export interface Progress {
  page: number;
  total: number;
}

// Re-exported from `@/lib/errors` so existing consumers of this wrapper
// (the tool's `types.ts` barrel and the e2e mock) keep their import paths
// unchanged after the shared-surfaces extraction.
export type { AppErrorEnvelope } from "../errors";

export type ConvertHooks = JobHooks<Progress>;

export async function convertPdfToImages(
  path: string,
  opts: Opts,
  hooks: ConvertHooks = {},
): Promise<JobResult> {
  return runJob<{ path: string; opts: Opts }, Progress, JobResult>(
    "convert_pdf_to_images",
    { path, opts },
    hooks,
  );
}
