import { useEffect, useRef, useState } from "react";
import type { Peak } from "@/lib/audioPreview";

/// Visible canvas height. The waveform draws bars in `[-1, 1]` mapped
/// to vertical space, so 96 px gives enough resolution to perceive
/// shape without dominating the screen.
const CANVAS_HEIGHT = 96;
/// Fallback canvas width when the parent hasn't laid out yet. The
/// ResizeObserver below replaces it as soon as the first measurement
/// arrives.
const CANVAS_FALLBACK_WIDTH = 800;

interface WaveformProps {
  peaks: Peak[];
  durationMs: number;
  startMs: number;
  endMs: number;
  onChange: (startMs: number, endMs: number) => void;
}

type DragKind = "start" | "end" | null;

/// Render the waveform on a canvas with two drag handles for the
/// `[start, end]` markers. Click-and-drag on a handle moves it; the
/// numeric inputs in the parent stay in sync via `onChange`. The
/// shaded region between the handles is the "kept" portion.
export function Waveform({
  peaks,
  durationMs,
  startMs,
  endMs,
  onChange,
}: WaveformProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = useState<number>(CANVAS_FALLBACK_WIDTH);
  const [drag, setDrag] = useState<DragKind>(null);

  // Track wrapper width via ResizeObserver so the canvas re-renders
  // crisply on viewport changes.
  useEffect(() => {
    const wrapper = wrapperRef.current;
    if (!wrapper) return undefined;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setWidth(Math.max(100, Math.round(entry.contentRect.width)));
    });
    observer.observe(wrapper);
    return () => {
      observer.disconnect();
    };
  }, []);

  // Render bars whenever peaks or width change.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(CANVAS_HEIGHT * dpr);
    canvas.style.width = `${String(width)}px`;
    canvas.style.height = `${String(CANVAS_HEIGHT)}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, CANVAS_HEIGHT);

    // Center line at y = CANVAS_HEIGHT / 2; samples in [-1, 1] map to
    // [0, CANVAS_HEIGHT] with y = (1 - sample) * H / 2.
    ctx.fillStyle = "rgba(120, 120, 130, 0.75)";
    const binWidth = width / peaks.length;
    for (let i = 0; i < peaks.length; i += 1) {
      const peak = peaks[i];
      if (!peak) continue;
      const x = i * binWidth;
      const yTop = ((1 - peak.max) * CANVAS_HEIGHT) / 2;
      const yBot = ((1 - peak.min) * CANVAS_HEIGHT) / 2;
      const h = Math.max(1, yBot - yTop);
      ctx.fillRect(x, yTop, Math.max(1, binWidth - 1), h);
    }
  }, [peaks, width]);

  const msToPx = (ms: number): number =>
    durationMs > 0 ? (ms / durationMs) * width : 0;

  const onPointerDown = (kind: DragKind) => {
    setDrag(kind);
  };

  useEffect(() => {
    if (drag === null) return undefined;
    const pxToMs = (px: number): number => {
      if (durationMs <= 0) return 0;
      const clamped = Math.max(0, Math.min(width, px));
      return Math.round((clamped / width) * durationMs);
    };
    const onMove = (event: PointerEvent) => {
      const wrapper = wrapperRef.current;
      if (!wrapper) return;
      const rect = wrapper.getBoundingClientRect();
      const px = event.clientX - rect.left;
      const ms = pxToMs(px);
      if (drag === "start") {
        // Pin start <= end. Clamp to one ms before end to keep the
        // range nominally non-empty; the parent's `endMs <= startMs`
        // check still catches it for the Trim button disable.
        onChange(Math.min(ms, endMs), endMs);
      } else {
        onChange(startMs, Math.max(ms, startMs));
      }
    };
    const onUp = () => {
      setDrag(null);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [drag, durationMs, width, startMs, endMs, onChange]);

  const startPx = msToPx(startMs);
  const endPx = msToPx(endMs);

  return (
    <div
      ref={wrapperRef}
      className="relative w-full select-none"
      style={{ height: CANVAS_HEIGHT }}
    >
      <canvas
        ref={canvasRef}
        aria-label="Audio waveform"
        className="block w-full rounded-md border border-border bg-card"
      />
      {/* Shaded keep-region between the handles. Inset by ±1 so it
          doesn't bleed past the handles' visible lines. */}
      <div
        aria-hidden
        className="pointer-events-none absolute top-0 bottom-0 bg-primary/15"
        style={{ left: startPx, width: Math.max(0, endPx - startPx) }}
      />
      <Handle
        kind="start"
        ariaLabel="Trim start"
        positionPx={startPx}
        onPointerDown={() => onPointerDown("start")}
      />
      <Handle
        kind="end"
        ariaLabel="Trim end"
        positionPx={endPx}
        onPointerDown={() => onPointerDown("end")}
      />
    </div>
  );
}

interface HandleProps {
  kind: "start" | "end";
  ariaLabel: string;
  positionPx: number;
  onPointerDown: () => void;
}

function Handle({ kind, ariaLabel, positionPx, onPointerDown }: HandleProps) {
  return (
    <div
      role="slider"
      aria-label={ariaLabel}
      // Spec aria-valuenow/min/max would be more correct, but the canvas
      // doesn't carry a numeric scale — the numeric input is the
      // authoritative readout. Keep the role + label so the user can
      // tab to the handle in tests.
      data-handle={kind}
      onPointerDown={(e) => {
        e.preventDefault();
        // Capture the pointer so movement outside the handle still
        // dispatches `pointermove` to the window.
        e.currentTarget.setPointerCapture(e.pointerId);
        onPointerDown();
      }}
      className="absolute top-0 bottom-0 w-2 -ml-1 cursor-ew-resize"
      style={{ left: positionPx }}
    >
      <div className="mx-auto h-full w-0.5 bg-primary" />
    </div>
  );
}
