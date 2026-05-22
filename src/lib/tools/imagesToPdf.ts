// IPC wrapper for the Images → PDF tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).
// The wrapper itself is intentionally thin — its only job is to name the
// Rust command and shape the args; mechanics (jobId, progress filter,
// abort wiring, unlisten) live in `runJob`.

import { runJob, type JobHooks } from "../jobRunner";

// Mirrors `multitool_core::tools::images_to_pdf::PageSize` with
// `#[serde(rename_all = "kebab-case")]` — `AutoFit` → "auto-fit", etc.
export type PageSize = "auto-fit" | "a4" | "letter";

// Field name matches the Rust struct (serde default = verbatim). Don't
// rename either side without changing both.
export interface Opts {
  page_size: PageSize;
}

// Mirrors `multitool_core::tools::images_to_pdf::JobResult`. Field names
// match the Rust serde output (snake_case) so this stays a thin shape
// adapter — no field renames to keep out of sync.
export interface JobResult {
  output_path: string;
  page_count: number;
  duration_ms: number;
}

export interface Progress {
  image: number;
  total: number;
}

// Re-exported from `@/lib/errors` so consumers of this wrapper keep their
// import paths consistent with pdfToImages.ts.
export type { AppErrorEnvelope } from "../errors";

export type ConvertHooks = JobHooks<Progress>;

export async function convertImagesToPdf(
  paths: string[],
  opts: Opts,
  hooks: ConvertHooks = {},
): Promise<JobResult> {
  return runJob<{ paths: string[]; opts: Opts }, Progress, JobResult>(
    "convert_images_to_pdf",
    { paths, opts },
    hooks,
  );
}
