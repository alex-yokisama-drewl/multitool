// Generic job-IPC runner shared by every tool wrapper under `src/lib/tools/`.
//
// Owns the mechanical bits that don't vary per tool:
//   1. JobId generation (crypto.randomUUID()) — used to filter shared
//      `tool:progress` events back to the caller that started them.
//   2. Listening for `tool:progress` and dispatching the payload to
//      `onProgress` only when `job_id` matches.
//   3. Wiring an `AbortSignal` to a `cancel_job` invoke. The Rust side
//      cooperatively cancels via the registered token (see
//      ARCHITECTURE.md §3.2) and rejects the original invoke with
//      `AppError::Cancelled`.
//   4. try/finally unsubscribe so the listener can't outlive the job
//      on either the happy or error path.
//
// Per-tool wrappers stay thin: they declare the command name, the args
// shape, and the progress / result types, then call `runJob`. See
// `src/lib/tools/pdfToImages.ts` for the canonical call site.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface JobHooks<Progress> {
  onProgress?: (progress: Progress) => void;
  signal?: AbortSignal;
}

interface ProgressEventPayload<Progress> {
  job_id: string;
  progress: Progress;
}

export async function runJob<Args, Progress, Result>(
  command: string,
  args: Args,
  { onProgress, signal }: JobHooks<Progress> = {},
): Promise<Result> {
  signal?.throwIfAborted();

  const jobId = crypto.randomUUID();

  const unlisten: UnlistenFn = await listen<ProgressEventPayload<Progress>>(
    "tool:progress",
    (event) => {
      if (event.payload.job_id !== jobId) return;
      onProgress?.(event.payload.progress);
    },
  );

  const onAbort = () => {
    void invoke("cancel_job", { jobId });
  };
  signal?.addEventListener("abort", onAbort, { once: true });

  try {
    return await invoke<Result>(command, { jobId, ...args });
  } finally {
    unlisten();
    signal?.removeEventListener("abort", onAbort);
  }
}
