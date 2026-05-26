// E2E-mode replacement for `src/lib/tools/audioTrimmer.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  Opts,
  Progress,
  JobResult,
  AppErrorEnvelope,
  TrimHooks,
} from "@/lib/tools/audioTrimmer";

import type { JobResult, Opts, TrimHooks } from "@/lib/tools/audioTrimmer";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export async function trimAudio(
  path: string,
  _opts: Opts,
  { onProgress, signal }: TrimHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  await new Promise((resolve) => setTimeout(resolve, 30));
  if (signal?.aborted) {
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    throw { kind: "Cancelled", message: "operation cancelled" };
  }
  onProgress?.({ kind: "started", source: path });

  // Derive the output name the same way the Rust side does:
  // `{stem}_trimmed.{ext}` next to the source.
  const lastSlash = path.lastIndexOf("/");
  const dir = lastSlash >= 0 ? path.slice(0, lastSlash) : MOCK_OUTPUT_DIR;
  const name = lastSlash >= 0 ? path.slice(lastSlash + 1) : path;
  const lastDot = name.lastIndexOf(".");
  const output =
    lastDot >= 0
      ? `${dir}/${name.slice(0, lastDot)}_trimmed${name.slice(lastDot)}`
      : `${dir}/${name}_trimmed`;

  return {
    output,
    warnings: [],
    duration_ms: 30,
  };
}
