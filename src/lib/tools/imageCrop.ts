// IPC wrapper for the Image Crop tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).
//
// Single-image shape: the picker is single-select, so the wrapper takes a
// `path: string`. The progress stream only has a `started` variant — a
// single-image failure becomes the rejected `runJob` promise carrying an
// `AppErrorEnvelope`.

import { runJob, type JobHooks } from "../jobRunner";

// Mirrors `multitool_core::tools::image_crop::CropRect`. `x`/`y` are signed:
// the frame can be dragged partly off-canvas and the backend clamps. The
// Rust shell receives this as the command's `opts` argument.
export interface CropRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

// Mirrors `multitool_core::tools::image_crop::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]`. Single variant today.
export interface Progress {
  kind: "started";
  source: string;
}

// Mirrors `JobResult` on the Rust side. `output` is the FILE path of the
// written crop — pass straight to `revealItemInDir`.
export interface JobResult {
  output: string;
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type CropHooks = JobHooks<Progress>;

export async function cropImage(
  path: string,
  rect: CropRect,
  hooks: CropHooks = {},
): Promise<JobResult> {
  return runJob<{ path: string; opts: CropRect }, Progress, JobResult>(
    "crop_image",
    { path, opts: rect },
    hooks,
  );
}
