// Thin re-exports of the IPC wrapper's types so the tool folder is
// self-describing (ADDING_A_TOOL §5).
export type {
  AppErrorEnvelope,
  DurationResult,
  JobResult,
  Opts,
  Progress,
  ProxyProgress,
  ProxyResult,
} from "@/lib/tools/videoTrimmer";
