import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { pickVideoFiles, revealInFolder } from "@/lib/system";
import { convertVideoFormat } from "@/lib/tools/videoFormatConverter";
import { fileName } from "@/lib/utils";
import type {
  AppErrorEnvelope,
  JobResult,
  Opts,
  Progress as ProgressEvent,
  TargetFormat,
} from "./types";

// State machine mirrors AudioFormatConverter exactly. Skip+continue
// means a job that ran at all returns Ok with a `skipped` list; only
// cancellation and orchestrator-level errors land in `staging.error`.
type ViewState =
  | { kind: "idle" }
  | { kind: "staging"; paths: string[]; error?: AppErrorEnvelope }
  | {
      kind: "running";
      paths: string[];
      current?: {
        source: string;
        index: number;
        total: number;
        fraction: number;
      };
    }
  | { kind: "done"; paths: string[]; result: JobResult };

export function VideoFormatConverter() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Options outlive state transitions so the user's choices survive
  // between picks / retries — same pattern as AudioFormatConverter.
  const [targetFormat, setTargetFormat] = useState<TargetFormat>("mp4");

  const abortRef = useRef<AbortController | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  // Each picker confirmation REPLACES the staged list (no append).
  // Cancelling the picker (null) leaves the current batch untouched.
  const pickVideos = async () => {
    const picked = await pickVideoFiles();
    if (!picked) return;
    setState({ kind: "staging", paths: picked });
  };

  const removePath = (path: string) => {
    setState((prev) => {
      if (prev.kind !== "staging") return prev;
      const next = prev.paths.filter((p) => p !== path);
      return next.length === 0
        ? { kind: "idle" }
        : { kind: "staging", paths: next };
    });
  };

  const buildOpts = (): Opts => ({ target_format: targetFormat });

  const convert = async (paths: string[]) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "running", paths });
    try {
      const result = await convertVideoFormat(paths, buildOpts(), {
        signal: controller.signal,
        onProgress: (progress: ProgressEvent) => {
          setState((prev) => {
            if (prev.kind !== "running") return prev;
            if (progress.kind === "started") {
              return {
                ...prev,
                current: {
                  source: progress.source,
                  index: progress.index,
                  total: progress.total,
                  fraction: 0,
                },
              };
            }
            if (progress.kind === "file-progress") {
              if (prev.current?.index !== progress.index) {
                return prev;
              }
              return {
                ...prev,
                current: { ...prev.current, fraction: progress.fraction },
              };
            }
            return prev;
          });
        },
      });
      setState({ kind: "done", paths, result });
    } catch (err) {
      const envelope = err as AppErrorEnvelope;
      setState({ kind: "staging", paths, error: envelope });
    } finally {
      abortRef.current = null;
    }
  };

  const cancel = () => abortRef.current?.abort();

  const reset = () => {
    setState({ kind: "idle" });
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Video Format Converter</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Convert one or more video files to MP4 (H.264 + AAC), WebM (VP9 +
          Opus), or Matroska (stream copy).
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void pickVideos()}>Select video files</Button>
      )}

      {state.kind === "staging" && (
        <div className="space-y-5">
          {state.error && (
            <div
              role="alert"
              className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
            >
              <div className="font-medium">{state.error.kind}</div>
              <div className="mt-1">{state.error.message}</div>
            </div>
          )}

          <div className="text-xs text-muted-foreground">
            Staged ({state.paths.length}). Output lands next to each input as{" "}
            <code>&lt;name&gt;_converted.&lt;ext&gt;</code>.
          </div>

          <ul role="list" aria-label="Staged video files" className="space-y-1">
            {state.paths.map((path) => (
              <li
                key={path}
                className="flex items-center justify-between rounded-md border border-border bg-card px-3 py-2"
              >
                <span className="truncate text-sm font-mono">
                  {fileName(path)}
                </span>
                <button
                  type="button"
                  aria-label={`Remove ${fileName(path)}`}
                  onClick={() => removePath(path)}
                  className="ml-2 inline-flex h-6 w-6 items-center justify-center rounded-full border border-border bg-background text-sm leading-none shadow-sm hover:bg-accent"
                >
                  ×
                </button>
              </li>
            ))}
          </ul>

          <fieldset className="space-y-3">
            <legend className="text-sm font-medium">Target format</legend>
            <RadioGroup
              value={targetFormat}
              onValueChange={(value) => setTargetFormat(value as TargetFormat)}
              className="flex flex-wrap gap-6"
            >
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-mp4" value="mp4" />
                <Label htmlFor="target-mp4">MP4 (H.264 + AAC)</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-webm" value="webm" />
                <Label htmlFor="target-webm">WebM (VP9 + Opus)</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-mkv" value="mkv" />
                <Label htmlFor="target-mkv">Matroska (stream copy)</Label>
              </div>
            </RadioGroup>
          </fieldset>

          <div className="flex gap-3">
            <Button
              onClick={() => void convert(state.paths)}
              disabled={state.paths.length === 0}
            >
              Convert
            </Button>
            <Button variant="outline" onClick={() => void pickVideos()}>
              Select different videos
            </Button>
          </div>
        </div>
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Converting</div>
            <div className="mt-1">
              {state.current ? (
                <>
                  {state.current.index + 1} / {state.current.total} —{" "}
                  <span className="font-medium">
                    {fileName(state.current.source)}
                  </span>
                </>
              ) : (
                <>
                  {state.paths.length}{" "}
                  {state.paths.length === 1 ? "file" : "files"}
                </>
              )}
            </div>
            {state.current && (
              <Progress
                className="mt-3"
                value={Math.round(state.current.fraction * 100)}
                aria-label={`Encoding progress: ${String(Math.round(state.current.fraction * 100))}%`}
              />
            )}
          </div>
          <Button variant="outline" onClick={cancel}>
            Cancel
          </Button>
        </div>
      )}

      {state.kind === "done" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Done</div>
            <div className="mt-1">
              {state.result.success_count} converted
              {state.result.skip_count > 0
                ? `, ${String(state.result.skip_count)} skipped`
                : ""}
              .
            </div>
          </div>
          {state.result.skipped.length > 0 && (
            <details className="rounded-md border border-border p-3 text-sm">
              <summary className="cursor-pointer text-sm font-medium">
                Skipped files ({state.result.skipped.length})
              </summary>
              <ul className="mt-2 space-y-2">
                {state.result.skipped.map((s) => (
                  <li key={s.source} className="text-xs">
                    <div className="font-mono">{fileName(s.source)}</div>
                    <div className="text-muted-foreground">
                      {s.error.kind}: {s.error.message}
                    </div>
                  </li>
                ))}
              </ul>
            </details>
          )}
          <div className="flex gap-3">
            {state.result.first_output_path !== null && (
              <Button
                onClick={() =>
                  void revealInFolder(state.result.first_output_path ?? "")
                }
              >
                Open output folder
              </Button>
            )}
            <Button variant="outline" onClick={reset}>
              Convert another
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
