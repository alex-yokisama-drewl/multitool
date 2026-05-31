// E2E-mode replacement for `src/lib/tools/imageCompress.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  AppErrorEnvelope,
  CompressedFile,
  CompressHooks,
  JobResult,
  Opts,
  Progress,
  SkippedFile,
} from "@/lib/tools/imageCompress";

import type {
  CompressedFile,
  CompressHooks,
  JobResult,
  Opts,
} from "@/lib/tools/imageCompress";

// Per-source extension → output extension. Mirrors the Rust strategy
// table; kept in the mock so the streamed outputs land at filenames the
// component (and the spec) can recognise.
function outputExt(source: string): string {
  const lastDot = source.lastIndexOf(".");
  if (lastDot < 0) return "webp";
  const ext = source.slice(lastDot + 1).toLowerCase();
  if (ext === "jpg" || ext === "jpeg") return ext;
  return "webp";
}

function outputPath(source: string): string {
  const lastSlash = source.lastIndexOf("/");
  const dir = lastSlash >= 0 ? source.slice(0, lastSlash) : "";
  const name = lastSlash >= 0 ? source.slice(lastSlash + 1) : source;
  const lastDot = name.lastIndexOf(".");
  const stem = lastDot > 0 ? name.slice(0, lastDot) : name;
  const ext = outputExt(source);
  return dir ? `${dir}/${stem}_compressed.${ext}` : `${stem}_compressed.${ext}`;
}

export async function compressImages(
  paths: string[],
  _opts: Opts,
  { onProgress, signal }: CompressHooks = {},
): Promise<JobResult> {
  void _opts;
  signal?.throwIfAborted();
  const outputs: CompressedFile[] = [];
  let firstOutput: string | null = null;
  for (let index = 0; index < paths.length; index++) {
    const source = paths[index] ?? "";
    await new Promise((resolve) => setTimeout(resolve, 30));
    if (signal?.aborted) {
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      throw { kind: "Cancelled", message: "operation cancelled" };
    }
    onProgress?.({ kind: "started", index, total: paths.length, source });
    const output = outputPath(source);
    // Deterministic "halved" sizes so the spec can assert on the -50%
    // delta in the done view.
    const original_bytes = 2_000_000;
    const compressed_bytes = 1_000_000;
    outputs.push({ source, output, original_bytes, compressed_bytes });
    firstOutput ??= output;
    onProgress?.({
      kind: "succeeded",
      index,
      total: paths.length,
      source,
      output,
      original_bytes,
      compressed_bytes,
    });
  }
  return {
    success_count: paths.length,
    skip_count: 0,
    outputs,
    skipped: [],
    first_output_path: firstOutput,
    duration_ms: 30 * paths.length,
  };
}
