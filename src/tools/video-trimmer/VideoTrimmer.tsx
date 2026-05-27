import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Pause, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { TimeInput } from "@/components/TimeInput";
import {
  allowMediaPreview,
  pickVideoFile,
  revealInFolder,
  videoAssetUrl,
} from "@/lib/system";
import { formatMs } from "@/lib/time";
import {
  cleanupPreviewProxy,
  preparePreviewProxy,
  probeVideoDuration,
  trimVideo,
} from "@/lib/tools/videoTrimmer";
import { probePlayable } from "@/lib/videoPreview";
import { fileName } from "@/lib/utils";
import { VideoScrubber } from "./VideoScrubber";
import type { AppErrorEnvelope, JobResult, Opts } from "./types";

// Rust → TS state machine. Only one file at a time, so a per-file failure
// becomes the rejected `trimVideo` promise and routes through the
// `picked.error` slot. `preparing` is the proxy-transcode phase that only
// runs when the WebView can't decode the source natively.
type ViewState =
  | { kind: "idle"; error?: AppErrorEnvelope }
  | { kind: "loading"; path: string }
  | { kind: "preparing"; path: string; fraction: number }
  | {
      kind: "picked";
      path: string;
      previewUrl: string;
      durationMs: number;
      error?: AppErrorEnvelope;
    }
  | {
      kind: "running";
      path: string;
      previewUrl: string;
      durationMs: number;
      fraction: number;
    }
  | { kind: "done"; result: JobResult };

function toEnvelope(err: unknown, fallback: string): AppErrorEnvelope {
  return typeof err === "object" && err !== null && "kind" in err
    ? (err as AppErrorEnvelope)
    : {
        kind: "ProcessingFailed",
        message: err instanceof Error ? err.message : fallback,
      };
}

export function VideoTrimmer() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Markers live outside ViewState so they survive picked ↔ running
  // transitions; re-initialised on each fresh pick.
  const [startMs, setStartMs] = useState(0);
  const [endMs, setEndMs] = useState(0);
  const [currentMs, setCurrentMs] = useState(0);
  const [playing, setPlaying] = useState(false);

  const abortRef = useRef<AbortController | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  // Path of the preview proxy currently on disk (if any), so we can clean
  // it up on the next pick / reset / unmount. `null` when the source plays
  // natively.
  const proxyRef = useRef<string | null>(null);

  const cleanupProxy = () => {
    const proxy = proxyRef.current;
    proxyRef.current = null;
    if (proxy) void cleanupPreviewProxy(proxy);
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  // Clean up any lingering proxy on unmount. Reads `proxyRef` directly so
  // the effect has no component-scope deps (and `cleanupPreviewProxy` is a
  // stable module import).
  useEffect(() => {
    return () => {
      const proxy = proxyRef.current;
      if (proxy) void cleanupPreviewProxy(proxy);
    };
  }, []);

  const stopPlayback = () => {
    videoRef.current?.pause();
    setPlaying(false);
  };

  const pick = async () => {
    stopPlayback();
    cleanupProxy();
    const picked = await pickVideoFile();
    if (!picked) return;
    setState({ kind: "loading", path: picked });
    try {
      const { duration_ms } = await probeVideoDuration(picked);
      await allowMediaPreview([picked]);
      const sourceUrl = videoAssetUrl(picked);

      let previewUrl = sourceUrl;
      if (!(await probePlayable(sourceUrl))) {
        // The WebView can't decode this source — transcode a proxy so the
        // player is never blind. Cancellable via the same abort path.
        const controller = new AbortController();
        abortRef.current = controller;
        setState({ kind: "preparing", path: picked, fraction: 0 });
        const { proxy_path } = await preparePreviewProxy(picked, {
          signal: controller.signal,
          onProgress: (p) => {
            setState((prev) =>
              prev.kind === "preparing"
                ? { ...prev, fraction: p.fraction }
                : prev,
            );
          },
        });
        proxyRef.current = proxy_path;
        previewUrl = videoAssetUrl(proxy_path);
      }

      setStartMs(0);
      setEndMs(duration_ms);
      setCurrentMs(0);
      setState({
        kind: "picked",
        path: picked,
        previewUrl,
        durationMs: duration_ms,
      });
    } catch (err) {
      const envelope = err as AppErrorEnvelope;
      // A cancelled proxy transcode returns the user to idle cleanly (no
      // error banner); anything else surfaces.
      if (
        typeof envelope === "object" &&
        envelope !== null &&
        envelope.kind === "Cancelled"
      ) {
        setState({ kind: "idle" });
      } else {
        setState({
          kind: "idle",
          error: toEnvelope(err, "failed to load video"),
        });
      }
    } finally {
      abortRef.current = null;
    }
  };

  const upperBound = (): number =>
    state.kind === "picked" ? state.durationMs : Number.POSITIVE_INFINITY;

  const seek = (ms: number) => {
    const video = videoRef.current;
    if (video) video.currentTime = ms / 1000;
    setCurrentMs(ms);
  };

  const onStartChange = (ms: number) => {
    stopPlayback();
    setStartMs(Math.max(0, Math.min(ms, endMs - 1, upperBound() - 1)));
  };
  const onEndChange = (ms: number) => {
    stopPlayback();
    setEndMs(Math.min(upperBound(), Math.max(ms, startMs + 1)));
  };
  const onRangeChange = (s: number, e: number) => {
    const upper = upperBound();
    const newStart = Math.max(0, Math.min(s, upper - 1));
    const newEnd = Math.min(upper, Math.max(e, newStart + 1));
    setStartMs(newStart);
    setEndMs(newEnd);
  };

  // Play only the selected window: seek to start, play, and pause once the
  // playhead reaches the end marker.
  const togglePlay = () => {
    const video = videoRef.current;
    if (!video) return;
    if (playing) {
      video.pause();
      setPlaying(false);
      return;
    }
    if (
      video.currentTime * 1000 < startMs ||
      video.currentTime * 1000 >= endMs
    ) {
      video.currentTime = startMs / 1000;
    }
    void video.play();
    setPlaying(true);
  };

  const onTimeUpdate = () => {
    const video = videoRef.current;
    if (!video) return;
    const ms = video.currentTime * 1000;
    setCurrentMs(ms);
    if (playing && ms >= endMs) {
      video.pause();
      setPlaying(false);
    }
  };

  const trim = async () => {
    if (state.kind !== "picked") return;
    stopPlayback();
    const controller = new AbortController();
    abortRef.current = controller;
    const running: ViewState = {
      kind: "running",
      path: state.path,
      previewUrl: state.previewUrl,
      durationMs: state.durationMs,
      fraction: 0,
    };
    setState(running);
    try {
      const result = await trimVideo(
        state.path,
        { start_ms: startMs, end_ms: endMs } satisfies Opts,
        {
          signal: controller.signal,
          onProgress: (p) => {
            if (p.kind === "file-progress") {
              setState((prev) =>
                prev.kind === "running"
                  ? { ...prev, fraction: p.fraction }
                  : prev,
              );
            }
          },
        },
      );
      cleanupProxy();
      setState({ kind: "done", result });
    } catch (err) {
      setState({
        kind: "picked",
        path: running.path,
        previewUrl: running.previewUrl,
        durationMs: running.durationMs,
        error: err as AppErrorEnvelope,
      });
    } finally {
      abortRef.current = null;
    }
  };

  const cancel = () => abortRef.current?.abort();

  const reset = () => {
    stopPlayback();
    cleanupProxy();
    setState({ kind: "idle" });
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Video Trimmer</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Trim a video to a range without re-encoding. Output preserves the
          source format and lands next to the input as{" "}
          <code>{"{stem}_trimmed.{ext}"}</code>. Cuts snap to the nearest
          keyframe, so the start may land slightly before your mark.
        </p>
      </header>

      {state.kind === "idle" && (
        <div className="space-y-3">
          {state.error && <ErrorAlert envelope={state.error} />}
          <Button onClick={() => void pick()}>Select video file</Button>
        </div>
      )}

      {state.kind === "loading" && (
        <div className="text-sm text-muted-foreground">
          Loading <span className="font-mono">{fileName(state.path)}</span>…
        </div>
      )}

      {state.kind === "preparing" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">
              Preparing preview
            </div>
            <div className="mt-1">
              This format isn&apos;t previewable directly, so we&apos;re
              transcoding a temporary preview of{" "}
              <span className="font-mono">{fileName(state.path)}</span>.
            </div>
            <Progress
              className="mt-3"
              value={Math.round(state.fraction * 100)}
              aria-label={`Preparing preview: ${String(Math.round(state.fraction * 100))}%`}
            />
          </div>
          <Button variant="outline" onClick={cancel}>
            Cancel
          </Button>
        </div>
      )}

      {state.kind === "picked" && (
        <div className="space-y-5">
          {state.error && <ErrorAlert envelope={state.error} />}

          <div className="text-sm">
            <div className="font-mono">{fileName(state.path)}</div>
            <div className="text-xs text-muted-foreground">
              Duration {formatMs(state.durationMs)}
            </div>
          </div>

          <video
            ref={videoRef}
            src={state.previewUrl}
            controls
            onTimeUpdate={onTimeUpdate}
            onEnded={() => setPlaying(false)}
            className="w-full rounded-md border border-border bg-black"
          />

          <VideoScrubber
            durationMs={state.durationMs}
            startMs={startMs}
            endMs={endMs}
            currentMs={currentMs}
            onChange={onRangeChange}
            onSeek={seek}
          />

          <div className="flex items-end justify-between gap-4">
            <TimeInput
              id="trim-start"
              label="Start"
              ms={startMs}
              max={state.durationMs}
              onChange={onStartChange}
            />
            <Button
              variant="outline"
              size="icon"
              onClick={togglePlay}
              aria-label={playing ? "Pause" : "Play selection"}
            >
              {playing ? <Pause /> : <Play />}
            </Button>
            <TimeInput
              id="trim-end"
              label="End"
              ms={endMs}
              max={state.durationMs}
              onChange={onEndChange}
              align="right"
            />
          </div>

          <div className="flex gap-3">
            <Button onClick={() => void trim()}>Trim</Button>
            <Button variant="outline" onClick={() => void pick()}>
              Pick different file
            </Button>
          </div>
        </div>
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Trimming</div>
            <div className="mt-1 font-medium font-mono">
              {fileName(state.path)}
            </div>
            <Progress
              className="mt-3"
              value={Math.round(state.fraction * 100)}
              aria-label={`Trim progress: ${String(Math.round(state.fraction * 100))}%`}
            />
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
              Trimmed to{" "}
              <span className="font-mono font-medium">
                {fileName(state.result.output)}
              </span>
              .
            </div>
          </div>
          <div className="flex gap-3">
            <Button onClick={() => void revealInFolder(state.result.output)}>
              Open output folder
            </Button>
            <Button variant="outline" onClick={reset}>
              Trim another
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

interface ErrorAlertProps {
  envelope: AppErrorEnvelope;
}

function ErrorAlert({ envelope }: ErrorAlertProps) {
  return (
    <div
      role="alert"
      className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
    >
      <div className="font-medium">{envelope.kind}</div>
      <div className="mt-1">{envelope.message}</div>
    </div>
  );
}
