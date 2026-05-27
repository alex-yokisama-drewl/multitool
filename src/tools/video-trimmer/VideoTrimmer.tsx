import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Pause, Play, Volume2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { TimeInput } from "@/components/TimeInput";
import { pickVideoFile, revealInFolder, videoAssetUrl } from "@/lib/system";
import { formatMs } from "@/lib/time";
import {
  cleanupPreviewProxy,
  cleanupStaleProxies,
  preparePreviewProxy,
  probeVideoDuration,
  trimVideo,
} from "@/lib/tools/videoTrimmer";
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
  const [volume, setVolume] = useState(1);

  const abortRef = useRef<AbortController | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  // The preview proxy temp file on disk + the object URL the player streams
  // from. Both are tied to the current pick and torn down together on the
  // next pick / reset / unmount.
  const proxyRef = useRef<string | null>(null);
  const blobUrlRef = useRef<string | null>(null);

  const teardownPreview = () => {
    if (blobUrlRef.current) {
      URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = null;
    }
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

  // Sweep proxies orphaned by a previous session on mount, and tear down
  // this session's proxy + object URL on unmount. Reads refs directly so
  // the effect has no component-scope deps.
  useEffect(() => {
    void cleanupStaleProxies();
    return () => {
      if (blobUrlRef.current) URL.revokeObjectURL(blobUrlRef.current);
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
    teardownPreview();
    const picked = await pickVideoFile();
    if (!picked) return;
    setState({ kind: "loading", path: picked });
    try {
      const { duration_ms } = await probeVideoDuration(picked);

      // Always transcode a small WebM preview proxy. We can't point the
      // <video> at the source's asset URL directly — WebKitGTK's media
      // pipeline won't load a custom protocol scheme — so we fetch the
      // proxy bytes and play them from an object URL, which every WebView
      // accepts. The trim itself still targets the original file.
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

      const response = await fetch(videoAssetUrl(proxy_path));
      if (!response.ok) {
        throw new Error(`preview fetch failed (${response.status.toString()})`);
      }
      const bytes = await response.arrayBuffer();
      const url = URL.createObjectURL(
        new Blob([bytes], { type: "video/webm" }),
      );
      blobUrlRef.current = url;

      setStartMs(0);
      setEndMs(duration_ms);
      setCurrentMs(0);
      setState({
        kind: "picked",
        path: picked,
        previewUrl: url,
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

  // Seek the player to `ms` (and reflect it in the playhead). Does not
  // pause — used by both the seek slider and the marker drag.
  const seekTo = (ms: number) => {
    const video = videoRef.current;
    const clamped = Math.max(0, ms);
    if (video) video.currentTime = clamped / 1000;
    setCurrentMs(clamped);
  };

  // Moving a marker stops + reset-seeks the preview so the user sees the
  // frame at the new edge instead of whatever was mid-play.
  const onStartChange = (ms: number) => {
    stopPlayback();
    const next = Math.max(0, Math.min(ms, endMs - 1, upperBound() - 1));
    setStartMs(next);
    seekTo(next);
  };
  const onEndChange = (ms: number) => {
    stopPlayback();
    const next = Math.min(upperBound(), Math.max(ms, startMs + 1));
    setEndMs(next);
    seekTo(next);
  };
  const onRangeChange = (s: number, e: number) => {
    stopPlayback();
    const upper = upperBound();
    const newStart = Math.max(0, Math.min(s, upper - 1));
    const newEnd = Math.min(upper, Math.max(e, newStart + 1));
    setStartMs(newStart);
    setEndMs(newEnd);
  };

  // Playback is clamped to the trim window: play starts at `startMs` (or
  // resumes within the window) and pauses when the playhead reaches
  // `endMs`, so the preview reflects exactly what gets trimmed.
  const togglePlay = () => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      const ms = video.currentTime * 1000;
      if (ms < startMs || ms >= endMs - 1) seekTo(startMs);
      void video.play();
    } else {
      video.pause();
    }
  };

  const onTimeUpdate = () => {
    const video = videoRef.current;
    if (!video) return;
    const ms = video.currentTime * 1000;
    if (ms >= endMs) {
      video.pause();
      seekTo(endMs);
      return;
    }
    setCurrentMs(ms);
  };

  const onVolumeChange = (pct: number) => {
    const vol = Math.max(0, Math.min(1, pct / 100));
    if (videoRef.current) videoRef.current.volume = vol;
    setVolume(vol);
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
      teardownPreview();
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
    teardownPreview();
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
            <div className="mt-1 font-medium font-mono">
              {fileName(state.path)}
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

          {/* No native `controls`: WebView control sets are inconsistent
              (fullscreen / playback-speed / download leak through), so we
              render a minimal bar with exactly what a trim preview needs. */}
          {/* No native `controls`: WebView control sets are inconsistent.
              Capped height so a vertical video stays small and the controls
              below it are visible without scrolling; object-contain
              letterboxes rather than stretches. */}
          <video
            ref={videoRef}
            src={state.previewUrl}
            playsInline
            preload="auto"
            onTimeUpdate={onTimeUpdate}
            onPlay={() => setPlaying(true)}
            onPause={() => setPlaying(false)}
            onLoadedMetadata={() => {
              if (videoRef.current) videoRef.current.volume = volume;
            }}
            className="mx-auto block max-h-[40vh] w-full rounded-md border border-border bg-black object-contain"
          />

          {/* Playback bar: play/pause + seek within the trim window + volume.
              The seek bar and time readout are clamped to [start, end] so the
              preview matches exactly what gets trimmed. */}
          <div className="flex items-center gap-3">
            <Button
              variant="outline"
              size="icon"
              onClick={togglePlay}
              aria-label={playing ? "Pause" : "Play"}
            >
              {playing ? <Pause /> : <Play />}
            </Button>
            <span className="whitespace-nowrap font-mono text-xs tabular-nums text-muted-foreground">
              {formatMs(Math.max(0, currentMs - startMs))} /{" "}
              {formatMs(endMs - startMs)}
            </span>
            <input
              type="range"
              aria-label="Seek"
              min={startMs}
              max={Math.max(startMs + 1, endMs)}
              value={Math.round(Math.min(Math.max(currentMs, startMs), endMs))}
              onChange={(e) => seekTo(Number(e.target.value))}
              className="flex-1 accent-primary"
            />
            <Volume2
              className="size-4 shrink-0 text-muted-foreground"
              aria-hidden
            />
            <input
              type="range"
              aria-label="Volume"
              min={0}
              max={100}
              value={Math.round(volume * 100)}
              onChange={(e) => onVolumeChange(Number(e.target.value))}
              className="w-20 accent-primary"
            />
          </div>

          {/* Trim window: draggable start/end markers (drag seeks the
              player so the edge frame is visible). */}
          <VideoScrubber
            durationMs={state.durationMs}
            startMs={startMs}
            endMs={endMs}
            onChange={onRangeChange}
            onSeek={seekTo}
          />

          <div className="flex items-end justify-between gap-4">
            <TimeInput
              id="trim-start"
              label="Start"
              ms={startMs}
              max={state.durationMs}
              onChange={onStartChange}
            />
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
