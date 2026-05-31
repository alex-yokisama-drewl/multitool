import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Pause, Play, Volume2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { pickVideoFile, revealInFolder, videoAssetUrl } from "@/lib/system";
import { formatMs } from "@/lib/time";
// The preview-proxy + duration-probe wrappers live in the trimmer's lib
// file — same Rust commands as ours (preview_proxy is app-level on the
// Rust side already), so duplicating the TS wrappers wouldn't buy
// anything. If a third video tool joins the picture, hoist these.
import {
  cleanupPreviewProxy,
  cleanupStaleProxies,
  preparePreviewProxy,
  probeVideoDuration,
} from "@/lib/tools/videoTrimmer";
import { compressVideo } from "@/lib/tools/videoCompress";
import { fileName } from "@/lib/utils";
import type { AppErrorEnvelope, JobResult, Opts } from "./types";

// Same Rust → TS state machine as the trimmer minus the trim-window
// inputs. `preparing` is the proxy-transcode phase the user sees a
// progress bar for; `picked` shows the player + the single quality
// slider; `running` is the compress-encode progress.
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

// Slider granularity: 1000 steps spans [0, 1] finely enough that the
// CRF-rounded backend value moves at most ~1 step per UI drag of a few
// pixels. (The Rust side rounds to an integer CRF anyway, so finer
// granularity past this just produces no-ops.)
const SLIDER_STEPS = 1000;
const DEFAULT_QUALITY = 0.5;

export function VideoCompress() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Slider value lives outside ViewState so it survives picked ↔ running
  // transitions; re-initialised only on full reset.
  const [quality, setQuality] = useState(DEFAULT_QUALITY);
  const [currentMs, setCurrentMs] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [volume, setVolume] = useState(1);

  const abortRef = useRef<AbortController | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
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

  // Sweep orphaned proxies on mount; tear down this session's proxy +
  // object URL on unmount. Reads refs directly so the effect has no
  // component-scope deps.
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

      setCurrentMs(0);
      setState({
        kind: "picked",
        path: picked,
        previewUrl: url,
        durationMs: duration_ms,
      });
    } catch (err) {
      const envelope = err as AppErrorEnvelope;
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

  const seekTo = (ms: number) => {
    const video = videoRef.current;
    const clamped = Math.max(0, ms);
    if (video) video.currentTime = clamped / 1000;
    setCurrentMs(clamped);
  };

  const togglePlay = () => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      void video.play();
    } else {
      video.pause();
    }
  };

  const onTimeUpdate = () => {
    const video = videoRef.current;
    if (!video) return;
    setCurrentMs(video.currentTime * 1000);
  };

  const onVolumeChange = (pct: number) => {
    const vol = Math.max(0, Math.min(1, pct / 100));
    if (videoRef.current) videoRef.current.volume = vol;
    setVolume(vol);
  };

  const compress = async () => {
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
      const result = await compressVideo(
        state.path,
        { quality } satisfies Opts,
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
        <h1 className="text-xl font-semibold">Video Compress</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Re-encode a video at a chosen quality to produce a smaller file.
          Output preserves the source codec and container, and lands next to the
          input as <code>{"{stem}_compressed.{ext}"}</code>.
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
              (fullscreen / playback-speed / download leak through). Capped
              height so a vertical video stays small and the slider below
              stays visible without scrolling. */}
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

          {/* Playback bar: play/pause + seek over the full clip + volume.
              No trim window — compress encodes the whole source. */}
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
              {formatMs(Math.max(0, currentMs))} / {formatMs(state.durationMs)}
            </span>
            <input
              type="range"
              aria-label="Seek"
              min={0}
              max={Math.max(1, state.durationMs)}
              value={Math.round(
                Math.max(0, Math.min(currentMs, state.durationMs)),
              )}
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

          {/* The single setting. Endpoints labeled; no number, no codec
              name, no estimated MB — quality scales differently per codec
              and an estimate would be visibly wrong on extreme content. */}
          <div className="space-y-2">
            <label htmlFor="compress-quality" className="text-sm font-medium">
              Quality
            </label>
            <div className="flex items-center gap-3">
              <span className="whitespace-nowrap text-xs text-muted-foreground">
                Smaller file
              </span>
              <input
                id="compress-quality"
                type="range"
                aria-label="Quality"
                min={0}
                max={SLIDER_STEPS}
                value={Math.round(quality * SLIDER_STEPS)}
                onChange={(e) =>
                  setQuality(Number(e.target.value) / SLIDER_STEPS)
                }
                className="flex-1 accent-primary"
              />
              <span className="whitespace-nowrap text-xs text-muted-foreground">
                Higher quality
              </span>
            </div>
          </div>

          <div className="flex gap-3">
            <Button onClick={() => void compress()}>Compress</Button>
            <Button variant="outline" onClick={() => void pick()}>
              Pick different file
            </Button>
          </div>
        </div>
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Compressing</div>
            <div className="mt-1 font-medium font-mono">
              {fileName(state.path)}
            </div>
            <Progress
              className="mt-3"
              value={Math.round(state.fraction * 100)}
              aria-label={`Compress progress: ${String(Math.round(state.fraction * 100))}%`}
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
              Compressed to{" "}
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
              Compress another
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
