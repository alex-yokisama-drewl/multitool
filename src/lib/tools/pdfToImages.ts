// IPC wrapper for the PDF → Images tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool live here so
// components stay presentational and Playwright can mock the IPC layer at
// the `src/lib/` seam (see `ARCHITECTURE.md` §6). Sets the pattern future
// tool wrappers will copy.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Format = "png" | "jpeg";

export interface Opts {
  format: Format;
  dpi: number;
}

// Mirrors `multitool_core::tools::pdf_to_images::JobResult`. Field names
// match the Rust serde output (snake_case) so this stays a thin shape
// adapter — no field renames to keep out of sync.
export interface JobResult {
  output_dir: string;
  page_count: number;
  duration_ms: number;
}

export interface Progress {
  page: number;
  total: number;
}

// Mirrors the `{ kind, message }` shape from `AppError`'s custom Serialize
// impl (`multitool-core/src/error.rs`). `kind` is the discriminant React
// components can branch on; `message` is the toast string.
export interface AppErrorEnvelope {
  kind:
    | "FileNotFound"
    | "PermissionDenied"
    | "UnsupportedFormat"
    | "ProcessingFailed"
    | "Encrypted"
    | "Cancelled";
  message: string;
}

export interface ConvertHooks {
  onProgress?: (progress: Progress) => void;
  signal?: AbortSignal;
}

interface ProgressEventPayload {
  job_id: string;
  progress: Progress;
}

export async function convertPdfToImages(
  path: string,
  opts: Opts,
  { onProgress, signal }: ConvertHooks = {},
): Promise<JobResult> {
  signal?.throwIfAborted();

  const jobId = crypto.randomUUID();

  const unlisten: UnlistenFn = await listen<ProgressEventPayload>(
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
    return await invoke<JobResult>("convert_pdf_to_images", {
      jobId,
      path,
      opts,
    });
  } finally {
    unlisten();
    signal?.removeEventListener("abort", onAbort);
  }
}
