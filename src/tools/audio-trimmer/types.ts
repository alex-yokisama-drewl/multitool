// Per-tool shared input/output contracts (mirrors Rust types).
// All shapes already live in the IPC wrapper at
// `@/lib/tools/audioTrimmer`; this file re-exports them so the tool
// folder is self-describing per ARCHITECTURE §3.1.

export type {
  Opts,
  Progress,
  JobResult,
  AppErrorEnvelope,
} from "@/lib/tools/audioTrimmer";
