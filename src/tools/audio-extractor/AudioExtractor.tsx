import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Progress as ProgressBar } from "@/components/ui/progress";
import { pickVideoFile, revealInFolder } from "@/lib/system";
import { extractAudio } from "@/lib/tools/audioExtractor";
import { fileName } from "@/lib/utils";
import type { AppErrorEnvelope, JobResult, Progress } from "./types";

// Single-file shape: idle → picked → running → done | error-on-picked
// (mirrors PdfToImages). No batch / no skip-and-continue. `current` in
// the running state carries the in-flight track's index + total + a
// 0..=1 fraction; the "Track N of M" label only renders when total > 1
// so single-track sources don't show a meaningless "Track 1 of 1".
type ViewState =
  | { kind: "idle" }
  | { kind: "picked"; path: string; error?: AppErrorEnvelope }
  | {
      kind: "running";
      path: string;
      current?: { index: number; total: number; fraction: number };
    }
  | { kind: "done"; path: string; result: JobResult };

export function AudioExtractor() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });
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

  const choose = async () => {
    const path = await pickVideoFile();
    if (path) setState({ kind: "picked", path });
  };

  const start = async (path: string) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "running", path });
    try {
      const result = await extractAudio(path, {
        signal: controller.signal,
        onProgress: (progress: Progress) => {
          setState((prev) => {
            if (prev.kind !== "running") return prev;
            if (progress.kind === "started") {
              return {
                ...prev,
                current: {
                  index: progress.index,
                  total: progress.total,
                  fraction: 0,
                },
              };
            }
            if (progress.kind === "file-progress") {
              if (prev.current?.index !== progress.index) return prev;
              return {
                ...prev,
                current: { ...prev.current, fraction: progress.fraction },
              };
            }
            return prev;
          });
        },
      });
      setState({ kind: "done", path, result });
    } catch (err) {
      const envelope = err as AppErrorEnvelope;
      setState({ kind: "picked", path, error: envelope });
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
        <h1 className="text-xl font-semibold">Audio Extractor</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Extract every audio track from a video file as MP3 (~190 kbps VBR).
          Output lands next to the source as <code>&lt;name&gt;_audio.mp3</code>
          , or <code>&lt;name&gt;_audio_1.mp3</code>, … for multi-track sources.
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void choose()}>Select video file</Button>
      )}

      {state.kind === "picked" && (
        <div className="space-y-5">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Input</div>
            <div className="mt-1 break-all font-medium">
              {fileName(state.path)}
            </div>
          </div>

          {state.error && (
            <div
              role="alert"
              className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
            >
              <div className="font-medium">{state.error.kind}</div>
              <div className="mt-1">{state.error.message}</div>
            </div>
          )}

          <div className="flex gap-3">
            <Button onClick={() => void start(state.path)}>
              Extract audio
            </Button>
            <Button variant="outline" onClick={() => void choose()}>
              Select a different video
            </Button>
          </div>
        </div>
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Extracting</div>
            <div className="mt-1 break-all">
              <span className="font-medium">{fileName(state.path)}</span>
              {state.current && state.current.total > 1 && (
                <>
                  {" "}
                  — Track {state.current.index + 1} of {state.current.total}
                </>
              )}
            </div>
            {state.current && (
              <ProgressBar
                className="mt-3"
                value={Math.round(state.current.fraction * 100)}
                aria-label={`Extraction progress: ${String(Math.round(state.current.fraction * 100))}%`}
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
              Extracted {state.result.track_count}{" "}
              {state.result.track_count === 1 ? "track" : "tracks"}.
            </div>
            <ul className="mt-2 space-y-1 text-xs">
              {state.result.outputs.map((output) => (
                <li key={output} className="break-all font-mono">
                  {fileName(output)}
                </li>
              ))}
            </ul>
          </div>
          <div className="flex gap-3">
            {state.result.outputs[0] !== undefined && (
              <Button
                onClick={() =>
                  void revealInFolder(state.result.outputs[0] ?? "")
                }
              >
                Open output folder
              </Button>
            )}
            <Button variant="outline" onClick={reset}>
              Extract another
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
