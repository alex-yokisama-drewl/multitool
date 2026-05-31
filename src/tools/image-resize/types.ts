// Per-tool shared input/output contracts (mirrors Rust types).
// All shapes already live in the IPC wrapper at `@/lib/tools/imageResize`;
// this file re-exports them so the tool folder is self-describing per
// ARCHITECTURE §3.1.

export type {
  AppErrorEnvelope,
  Background,
  FitMode,
  JobResult,
  Opts,
  Progress,
} from "@/lib/tools/imageResize";
