// IPC wrapper for the Audio Format Converter tool.
//
// Boundary file: all `@tauri-apps/api` calls for this tool route through
// `runJob` (see `../jobRunner.ts`) so components stay presentational and
// Playwright can mock at the `src/lib/` seam (ARCHITECTURE.md §6).

import { runJob, type JobHooks } from "../jobRunner";
import type { AppErrorEnvelope } from "../errors";

// Mirrors `multitool_core::tools::audio_format_converter::TargetFormat`
// with `#[serde(rename_all = "lowercase")]`. Keep variants in sync — the
// Rust side gates encoder selection on these strings.
export type TargetFormat = "wav" | "flac" | "mp3" | "ogg";

// Mirrors `WavBitDepth` (`#[serde(rename_all = "kebab-case")]`).
export type WavBitDepth = "bit16" | "bit24" | "bit32f";

// Mirrors `ChannelMode` (`#[serde(rename_all = "kebab-case")]`).
export type ChannelMode = "source" | "mono" | "stereo";

export interface Opts {
  target_format: TargetFormat;
  mp3_bitrate_kbps: number;
  vorbis_quality: number;
  flac_compression_level: number;
  wav_bit_depth: WavBitDepth;
  channels: ChannelMode;
}

// Mirrors `multitool_core::tools::audio_format_converter::Progress` —
// `#[serde(tag = "kind", rename_all = "kebab-case")]` flattens each
// variant's fields next to a `kind` discriminator.
export type Progress =
  | {
      kind: "started";
      index: number;
      total: number;
      source: string;
    }
  | {
      kind: "succeeded";
      index: number;
      total: number;
      source: string;
      output: string;
      warnings: string[];
    }
  | {
      kind: "skipped";
      index: number;
      total: number;
      source: string;
      error: AppErrorEnvelope;
    };

export interface SkippedFile {
  source: string;
  error: AppErrorEnvelope;
}

// Mirrors `JobResult`. `first_output_path` is the FILE path of the first
// successful output — pass it straight to `revealItemInDir`. `null` when
// no file succeeded.
export interface JobResult {
  success_count: number;
  skip_count: number;
  skipped: SkippedFile[];
  first_output_path: string | null;
  duration_ms: number;
}

export type { AppErrorEnvelope } from "../errors";

export type ConvertHooks = JobHooks<Progress>;

export async function convertAudioFormat(
  paths: string[],
  opts: Opts,
  hooks: ConvertHooks = {},
): Promise<JobResult> {
  return runJob<{ paths: string[]; opts: Opts }, Progress, JobResult>(
    "convert_audio_format",
    { paths, opts },
    hooks,
  );
}
