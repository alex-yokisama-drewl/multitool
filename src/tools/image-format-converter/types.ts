// Per-tool shared input/output contracts (mirrors Rust types).
// All shapes already live in the IPC wrapper at
// `@/lib/tools/imageFormatConverter`; this file re-exports them so the
// tool folder is self-describing per ARCHITECTURE §3.1.

export type {
  TargetFormat,
  AlphaHandling,
  SvgRasterSize,
  Opts,
  Progress,
  SkippedFile,
  JobResult,
  AppErrorEnvelope,
} from "@/lib/tools/imageFormatConverter";
