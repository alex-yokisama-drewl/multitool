// E2E-mode replacement for `src/lib/tools/videoCompress.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.
//
// The preview-proxy + duration-probe wrappers live in
// `tests/e2e/mocks/videoTrimmer.ts` and are aliased separately — the
// VideoCompress component imports them from `@/lib/tools/videoTrimmer`.

export type {
  Opts,
  Progress,
  JobResult,
  AppErrorEnvelope,
  CompressHooks,
} from "@/lib/tools/videoCompress";

import type { CompressHooks, JobResult, Opts } from "@/lib/tools/videoCompress";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export async function compressVideo(
  path: string,
  _opts: Opts,
  { onProgress, signal }: CompressHooks = {},
): Promise<JobResult> {
  void _opts;
  signal?.throwIfAborted();
  onProgress?.({ kind: "started", source: path });
  await new Promise((resolve) => setTimeout(resolve, 30));
  if (signal?.aborted) {
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    throw { kind: "Cancelled", message: "operation cancelled" };
  }
  onProgress?.({ kind: "file-progress", source: path, fraction: 0.5 });

  // Derive `{stem}_compressed.{ext}` next to the source, like the Rust side.
  const lastSlash = path.lastIndexOf("/");
  const dir = lastSlash >= 0 ? path.slice(0, lastSlash) : MOCK_OUTPUT_DIR;
  const name = lastSlash >= 0 ? path.slice(lastSlash + 1) : path;
  const lastDot = name.lastIndexOf(".");
  const output =
    lastDot >= 0
      ? `${dir}/${name.slice(0, lastDot)}_compressed${name.slice(lastDot)}`
      : `${dir}/${name}_compressed`;

  return { output, duration_ms: 30 };
}
