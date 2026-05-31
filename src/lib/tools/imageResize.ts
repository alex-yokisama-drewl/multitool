// IPC wrapper for the Image Resize tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).
//
// Single-image shape (same as imageCrop.ts): the picker is single-select,
// so the wrapper takes a `path: string`. The progress stream only has a
// `started` variant — a single-image failure becomes the rejected `runJob`
// promise carrying an `AppErrorEnvelope`.

import { runJob, type JobHooks } from "../jobRunner";

// Mirrors `multitool_core::tools::image_resize::FitMode` —
// `#[serde(rename_all = "kebab-case")]` over single-word variants leaves
// them lowercase.
export type FitMode = "stretch" | "contain" | "cover";

// Mirrors `multitool_core::tools::image_resize::Background`.
export type Background = "white" | "black" | "transparent";

// Mirrors `multitool_core::tools::image_resize::Opts`. The Rust shell
// receives this as the command's `opts` argument; backend clamps width /
// height into `[1, MAX_DIM]` so anything the user types is accepted.
export interface Opts {
  width: number;
  height: number;
  fit: FitMode;
  background: Background;
}

// Mirrors `multitool_core::tools::image_resize::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]`. Single variant today.
export interface Progress {
  kind: "started";
  source: string;
}

// Mirrors `JobResult` on the Rust side. `output` is the FILE path of the
// written resized image — pass straight to `revealItemInDir`.
export interface JobResult {
  output: string;
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type ResizeHooks = JobHooks<Progress>;

export async function resizeImage(
  path: string,
  opts: Opts,
  hooks: ResizeHooks = {},
): Promise<JobResult> {
  return runJob<{ path: string; opts: Opts }, Progress, JobResult>(
    "resize_image",
    { path, opts },
    hooks,
  );
}
