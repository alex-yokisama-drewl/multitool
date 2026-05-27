import { useEffect, useRef, useState } from "react";

interface VideoScrubberProps {
  durationMs: number;
  startMs: number;
  endMs: number;
  /// Live playhead position (the player's current time), drawn as a thin
  /// marker so the user can see where playback is relative to the window.
  currentMs: number;
  /// Fired continuously while a handle is dragged. The parent clamps and
  /// updates the markers.
  onChange: (startMs: number, endMs: number) => void;
  /// Fired with the dragged handle's time so the parent can seek the
  /// player — this is what makes the corresponding frame show under the
  /// handle instead of trimming blind.
  onSeek: (ms: number) => void;
}

type DragKind = "start" | "end" | null;

/// A plain horizontal timeline (no filmstrip) carrying the trim window:
/// a shaded keep-region between two draggable handles, plus a playhead.
/// Mirrors the Audio Trimmer's `Waveform` drag mechanics, minus the canvas
/// — the video element above is the visual, this is just the range picker.
export function VideoScrubber({
  durationMs,
  startMs,
  endMs,
  currentMs,
  onChange,
  onSeek,
}: VideoScrubberProps) {
  const trackRef = useRef<HTMLDivElement | null>(null);
  const [drag, setDrag] = useState<DragKind>(null);

  const pct = (ms: number): number =>
    durationMs > 0 ? Math.max(0, Math.min(100, (ms / durationMs) * 100)) : 0;

  useEffect(() => {
    if (drag === null) return undefined;
    const pxToMs = (clientX: number): number => {
      const track = trackRef.current;
      if (!track || durationMs <= 0) return 0;
      const rect = track.getBoundingClientRect();
      const ratio = (clientX - rect.left) / Math.max(1, rect.width);
      return Math.round(Math.max(0, Math.min(1, ratio)) * durationMs);
    };
    const onMove = (event: PointerEvent) => {
      const ms = pxToMs(event.clientX);
      if (drag === "start") {
        const next = Math.min(ms, endMs - 1);
        onChange(next, endMs);
        onSeek(next);
      } else {
        const next = Math.max(ms, startMs + 1);
        onChange(startMs, next);
        onSeek(next);
      }
    };
    const onUp = () => setDrag(null);
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [drag, durationMs, startMs, endMs, onChange, onSeek]);

  return (
    <div
      ref={trackRef}
      className="relative h-10 w-full select-none rounded-md border border-border bg-card"
    >
      {/* Shaded keep-region between the handles. */}
      <div
        aria-hidden
        className="pointer-events-none absolute top-0 bottom-0 bg-primary/15"
        style={{
          left: `${String(pct(startMs))}%`,
          width: `${String(Math.max(0, pct(endMs) - pct(startMs)))}%`,
        }}
      />
      {/* Playhead. */}
      <div
        aria-hidden
        className="pointer-events-none absolute top-0 bottom-0 w-px bg-foreground/70"
        style={{ left: `${String(pct(currentMs))}%` }}
      />
      <Handle
        kind="start"
        ariaLabel="Trim start"
        leftPct={pct(startMs)}
        onGrab={() => setDrag("start")}
      />
      <Handle
        kind="end"
        ariaLabel="Trim end"
        leftPct={pct(endMs)}
        onGrab={() => setDrag("end")}
      />
    </div>
  );
}

interface HandleProps {
  kind: "start" | "end";
  ariaLabel: string;
  leftPct: number;
  onGrab: () => void;
}

function Handle({ kind, ariaLabel, leftPct, onGrab }: HandleProps) {
  return (
    <div
      role="slider"
      aria-label={ariaLabel}
      data-handle={kind}
      onPointerDown={(e) => {
        e.preventDefault();
        // Capture so movement outside the handle still reaches the window
        // pointermove listener.
        e.currentTarget.setPointerCapture(e.pointerId);
        onGrab();
      }}
      className="absolute top-0 bottom-0 w-3 -ml-1.5 cursor-ew-resize"
      style={{ left: `${String(leftPct)}%` }}
    >
      <div className="mx-auto h-full w-1 rounded bg-primary" />
    </div>
  );
}
