// E2E-mode replacement for `src/lib/tools/audioExtractor.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.
//
// Returns track_count = 2 so the spec can also exercise the multi-track
// branch in the running view (the "Track N of M" label).

export type {
  Progress,
  JobResult,
  AppErrorEnvelope,
  ExtractHooks,
} from "@/lib/tools/audioExtractor";

import type { ExtractHooks, JobResult } from "@/lib/tools/audioExtractor";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export async function extractAudio(
  path: string,
  { onProgress, signal }: ExtractHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  const total = 2;
  const outputs: string[] = [];
  for (let index = 0; index < total; index++) {
    await new Promise((resolve) => setTimeout(resolve, 20));
    if (signal?.aborted) {
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      throw { kind: "Cancelled", message: "operation cancelled" };
    }
    onProgress?.({ kind: "started", index, total });
    // One mid-track progress sample so the bar renders meaningfully —
    // matches the videoFormatConverter mock's "one sample per file" rule.
    onProgress?.({ kind: "file-progress", index, total, fraction: 0.5 });
    const output = `${MOCK_OUTPUT_DIR}/${stem(path)}_audio_${String(index + 1)}.mp3`;
    outputs.push(output);
    onProgress?.({ kind: "succeeded", index, total, output });
  }
  return {
    track_count: total,
    outputs,
    duration_ms: 20 * total,
  };
}

function stem(p: string): string {
  const base = p.split(/[\\/]/).pop() ?? p;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}
