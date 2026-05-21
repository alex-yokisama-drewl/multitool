// E2E-mode replacement for `src/lib/tools/pdfToImages.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true
// is set on the dev server's env (Playwright's webServer block sets it).
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  Format,
  Opts,
  JobResult,
  Progress,
  AppErrorEnvelope,
  ConvertHooks,
} from "@/lib/tools/pdfToImages";

import type { ConvertHooks, JobResult, Opts } from "@/lib/tools/pdfToImages";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e/sample_pages";
const MOCK_PAGE_COUNT = 3;

export async function convertPdfToImages(
  _path: string,
  _opts: Opts,
  { onProgress, signal }: ConvertHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();
  for (let page = 1; page <= MOCK_PAGE_COUNT; page++) {
    await new Promise((resolve) => setTimeout(resolve, 30));
    if (signal?.aborted) {
      // Mirror the real wrapper's wire shape (plain envelope, not Error).
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      throw { kind: "Cancelled", message: "operation cancelled" };
    }
    onProgress?.({ page, total: MOCK_PAGE_COUNT });
  }
  return {
    output_dir: MOCK_OUTPUT_DIR,
    page_count: MOCK_PAGE_COUNT,
    duration_ms: 100,
  };
}
