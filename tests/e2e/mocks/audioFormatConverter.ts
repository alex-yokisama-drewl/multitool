// E2E-mode replacement for `src/lib/tools/audioFormatConverter.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  TargetFormat,
  WavBitDepth,
  ChannelMode,
  Opts,
  Progress,
  SkippedFile,
  JobResult,
  AppErrorEnvelope,
  ConvertHooks,
} from "@/lib/tools/audioFormatConverter";

import type {
  ConvertHooks,
  JobResult,
  Opts,
} from "@/lib/tools/audioFormatConverter";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export async function convertAudioFormat(
  paths: string[],
  _opts: Opts,
  { onProgress, signal }: ConvertHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  let firstOutput: string | null = null;
  for (let index = 0; index < paths.length; index++) {
    const source = paths[index] ?? "";
    await new Promise((resolve) => setTimeout(resolve, 30));
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
    const output = `${MOCK_OUTPUT_DIR}/converted-${String(index)}.mp3`;
    firstOutput ??= output;
    onProgress?.({
      kind: "succeeded",
      index,
      total: paths.length,
      source,
      output,
      warnings: [],
    });
  }
  return {
    success_count: paths.length,
    skip_count: 0,
    skipped: [],
    first_output_path: firstOutput,
    duration_ms: 30 * paths.length,
  };
}
