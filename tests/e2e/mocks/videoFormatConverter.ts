// E2E-mode replacement for `src/lib/tools/videoFormatConverter.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  TargetFormat,
  Opts,
  Progress,
  SkippedFile,
  JobResult,
  AppErrorEnvelope,
  ConvertHooks,
} from "@/lib/tools/videoFormatConverter";

import type {
  ConvertHooks,
  JobResult,
  Opts,
} from "@/lib/tools/videoFormatConverter";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export async function convertVideoFormat(
  paths: string[],
  opts: Opts,
  { onProgress, signal }: ConvertHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  let firstOutput: string | null = null;
  for (let index = 0; index < paths.length; index++) {
    const source = paths[index] ?? "";
    await new Promise((resolve) => setTimeout(resolve, 20));
    if (signal?.aborted) {
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      throw { kind: "Cancelled", message: "operation cancelled" };
    }
    onProgress?.({
      kind: "started",
      index,
      total: paths.length,
      source,
    });
    // Surface a single mid-file progress event so the progress bar in
    // the running view renders meaningfully without slowing the spec
    // down. The real shim throttles to ~250 ms; one sample per file is
    // enough to drive the UI.
    onProgress?.({
      kind: "file-progress",
      index,
      total: paths.length,
      source,
      fraction: 0.5,
    });
    const output = `${MOCK_OUTPUT_DIR}/converted-${String(index)}.${opts.target_format}`;
    firstOutput ??= output;
    onProgress?.({
      kind: "succeeded",
      index,
      total: paths.length,
      source,
      output,
    });
  }
  return {
    success_count: paths.length,
    skip_count: 0,
    skipped: [],
    first_output_path: firstOutput,
    duration_ms: 20 * paths.length,
  };
}
