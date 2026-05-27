// IPC wrapper for the Video Trimmer tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// here so components stay presentational and Playwright can mock at the
// `src/lib/` seam (ARCHITECTURE.md §6).
//
// Two streaming jobs (trim + proxy transcode) go through `runJob`; two
// one-shot queries (duration probe + proxy cleanup) are plain `invoke`s.

import { invoke } from "@tauri-apps/api/core";
import { runJob, type JobHooks } from "../jobRunner";
import type { AppErrorEnvelope } from "../errors";

// Mirrors `multitool_core::tools::video_trimmer::Opts`. Range bounds are
// milliseconds from the start of the source; `end_ms` is clamped to the
// probed duration on the Rust side.
export interface Opts {
  start_ms: number;
  end_ms: number;
}

// Mirrors `multitool_core::tools::video_trimmer::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]`. Single file, so no
// index/total: `started` once, then `file-progress` with the mid-copy
// 0..=1 fraction (a stream copy is usually near-instant, so few or none).
export type Progress =
  | { kind: "started"; source: string }
  | { kind: "file-progress"; source: string; fraction: number };

// Mirrors `JobResult`. `output` is the FILE path of the trimmed clip —
// pass straight to `revealInFolder`.
export interface JobResult {
  output: string;
  duration_ms: number;
}

// Mirrors the shell's `ProxyProgress` / `ProxyResult`. The proxy transcode
// streams a single `fraction` field (no `kind` tag — it's not an enum).
export interface ProxyProgress {
  fraction: number;
}
export interface ProxyResult {
  proxy_path: string;
}

// Mirrors the shell's `DurationResult`.
export interface DurationResult {
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type TrimHooks = JobHooks<Progress>;
export type ProxyHooks = JobHooks<ProxyProgress>;

export async function trimVideo(
  path: string,
  opts: Opts,
  hooks: TrimHooks = {},
): Promise<JobResult> {
  return runJob<{ path: string; opts: Opts }, Progress, JobResult>(
    "trim_video",
    { path, opts },
    hooks,
  );
}

// Transcode a web-friendly preview proxy for a source the WebView can't
// decode. Cancellable + progress-reporting; resolves with the proxy path
// (already granted asset-protocol scope by the backend).
export async function preparePreviewProxy(
  path: string,
  hooks: ProxyHooks = {},
): Promise<ProxyResult> {
  return runJob<{ path: string }, ProxyProgress, ProxyResult>(
    "prepare_preview_proxy",
    { path },
    hooks,
  );
}

// One-shot duration probe so the picked view can size the trim window
// before running. Rejects with an `AppErrorEnvelope` on a missing /
// unreadable source.
export async function probeVideoDuration(
  path: string,
): Promise<DurationResult> {
  return invoke<DurationResult>("probe_video_duration", { path });
}

// Best-effort delete of a preview proxy once the user moves on. Backend
// guards the path to the temp dir + our prefix, so a bad path rejects with
// an `AppErrorEnvelope` rather than unlinking anything.
export async function cleanupPreviewProxy(path: string): Promise<void> {
  await invoke<void>("cleanup_preview_proxy", { path });
}

// Re-export so callers don't reach back into `../errors` directly.
export type VideoTrimmerError = AppErrorEnvelope;
