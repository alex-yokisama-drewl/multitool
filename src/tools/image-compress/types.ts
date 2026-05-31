// Per-tool shared input/output contracts (mirrors Rust types).
// All shapes already live in the IPC wrapper at
// `@/lib/tools/imageCompress`; this file re-exports them so the tool
// folder is self-describing per ARCHITECTURE §3.1.

export type {
  AppErrorEnvelope,
  CompressedFile,
  JobResult,
  Opts,
  Progress,
  SkippedFile,
} from "@/lib/tools/imageCompress";
