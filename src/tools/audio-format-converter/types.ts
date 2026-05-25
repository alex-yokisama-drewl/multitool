// Per-tool shared input/output contracts (mirrors Rust types).
// All shapes already live in the IPC wrapper at
// `@/lib/tools/audioFormatConverter`; this file re-exports them so the
// tool folder is self-describing per ARCHITECTURE §3.1.

export type {
  TargetFormat,
  WavBitDepth,
  ChannelMode,
  Opts,
  Progress,
  SkippedFile,
  JobResult,
  AppErrorEnvelope,
} from "@/lib/tools/audioFormatConverter";
