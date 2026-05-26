// E2E-mode replacement for `src/lib/tools/imageCrop.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  CropRect,
  Progress,
  JobResult,
  AppErrorEnvelope,
  CropHooks,
} from "@/lib/tools/imageCrop";

import type { CropHooks, CropRect, JobResult } from "@/lib/tools/imageCrop";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export async function cropImage(
  path: string,
  _rect: CropRect,
  { onProgress, signal }: CropHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  await new Promise((resolve) => setTimeout(resolve, 30));
  if (signal?.aborted) {
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    throw { kind: "Cancelled", message: "operation cancelled" };
  }
  onProgress?.({ kind: "started", source: path });

  // Derive `{stem}_cropped.{ext}` next to the source, like the Rust side.
  const lastSlash = path.lastIndexOf("/");
  const dir = lastSlash >= 0 ? path.slice(0, lastSlash) : MOCK_OUTPUT_DIR;
  const name = lastSlash >= 0 ? path.slice(lastSlash + 1) : path;
  const lastDot = name.lastIndexOf(".");
  const output =
    lastDot >= 0
      ? `${dir}/${name.slice(0, lastDot)}_cropped${name.slice(lastDot)}`
      : `${dir}/${name}_cropped`;

  return { output, duration_ms: 30 };
}
