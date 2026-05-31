// E2E-mode replacement for `src/lib/tools/imageResize.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  AppErrorEnvelope,
  Background,
  FitMode,
  JobResult,
  Opts,
  Progress,
  ResizeHooks,
} from "@/lib/tools/imageResize";

import type { JobResult, Opts, ResizeHooks } from "@/lib/tools/imageResize";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export async function resizeImage(
  path: string,
  _opts: Opts,
  { onProgress, signal }: ResizeHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  await new Promise((resolve) => setTimeout(resolve, 30));
  if (signal?.aborted) {
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    throw { kind: "Cancelled", message: "operation cancelled" };
  }
  onProgress?.({ kind: "started", source: path });

  // Derive `{stem}_resized.{ext}` next to the source, like the Rust side.
  const lastSlash = path.lastIndexOf("/");
  const dir = lastSlash >= 0 ? path.slice(0, lastSlash) : MOCK_OUTPUT_DIR;
  const name = lastSlash >= 0 ? path.slice(lastSlash + 1) : path;
  const lastDot = name.lastIndexOf(".");
  const output =
    lastDot >= 0
      ? `${dir}/${name.slice(0, lastDot)}_resized${name.slice(lastDot)}`
      : `${dir}/${name}_resized`;

  return { output, duration_ms: 30 };
}
