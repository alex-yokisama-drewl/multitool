// E2E-mode replacement for `src/lib/tools/videoTrimmer.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Module shape MUST match the real wrapper — TypeScript catches drift.

export type {
  Opts,
  Progress,
  JobResult,
  ProxyProgress,
  ProxyResult,
  DurationResult,
  AppErrorEnvelope,
  TrimHooks,
  ProxyHooks,
} from "@/lib/tools/videoTrimmer";

import type {
  DurationResult,
  JobResult,
  Opts,
  ProxyHooks,
  ProxyResult,
  TrimHooks,
} from "@/lib/tools/videoTrimmer";

const MOCK_OUTPUT_DIR = "/tmp/multitool-e2e";

export function probeVideoDuration(_path: string): Promise<DurationResult> {
  void _path;
  return Promise.resolve({ duration_ms: 12_000 });
}

export async function preparePreviewProxy(
  _path: string,
  { onProgress, signal }: ProxyHooks = {},
): Promise<ProxyResult> {
  void _path;
  signal?.throwIfAborted();
  onProgress?.({ fraction: 0.5 });
  await new Promise((resolve) => setTimeout(resolve, 20));
  if (signal?.aborted) {
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    throw { kind: "Cancelled", message: "operation cancelled" };
  }
  return { proxy_path: `${MOCK_OUTPUT_DIR}/multitool-preview-e2e.webm` };
}

export async function trimVideo(
  path: string,
  _opts: Opts,
  { onProgress, signal }: TrimHooks = {},
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

  // Derive `{stem}_trimmed.{ext}` next to the source, like the Rust side.
  const lastSlash = path.lastIndexOf("/");
  const dir = lastSlash >= 0 ? path.slice(0, lastSlash) : MOCK_OUTPUT_DIR;
  const name = lastSlash >= 0 ? path.slice(lastSlash + 1) : path;
  const lastDot = name.lastIndexOf(".");
  const output =
    lastDot >= 0
      ? `${dir}/${name.slice(0, lastDot)}_trimmed${name.slice(lastDot)}`
      : `${dir}/${name}_trimmed`;

  return { output, duration_ms: 30 };
}

export function cleanupPreviewProxy(_path: string): Promise<void> {
  void _path;
  return Promise.resolve();
}

export function cleanupStaleProxies(): Promise<void> {
  return Promise.resolve();
}
