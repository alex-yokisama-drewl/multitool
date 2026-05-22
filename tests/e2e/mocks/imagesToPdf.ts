// E2E-mode replacement for `src/lib/tools/imagesToPdf.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true
// is set on the dev server's env (Playwright's webServer block sets it).
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  PageSize,
  Opts,
  JobResult,
  Progress,
  AppErrorEnvelope,
  ConvertHooks,
} from "@/lib/tools/imagesToPdf";

import type { ConvertHooks, JobResult, Opts } from "@/lib/tools/imagesToPdf";

const MOCK_OUTPUT_PATH = "/tmp/multitool-e2e/alpha.pdf";

export async function convertImagesToPdf(
  paths: string[],
  _opts: Opts,
  { onProgress, signal }: ConvertHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  // Stream one progress event per input image, mirroring the real
  // wrapper's per-image cadence.
  for (let image = 1; image <= paths.length; image++) {
    await new Promise((resolve) => setTimeout(resolve, 30));
    if (signal?.aborted) {
      // Mirror the real wrapper's wire shape (plain envelope, not Error).
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      throw { kind: "Cancelled", message: "operation cancelled" };
    }
    onProgress?.({ image, total: paths.length });
  }
  return {
    output_path: MOCK_OUTPUT_PATH,
    page_count: paths.length,
    duration_ms: 100,
  };
}
