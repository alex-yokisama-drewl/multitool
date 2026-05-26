import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Pause, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  createPreviewPlayer,
  loadAudioPreview,
  type AudioPreviewSource,
  type PreviewPlayer,
} from "@/lib/audioPreview";
import { allowMediaPreview, pickAudioFile, revealInFolder } from "@/lib/system";
import { trimAudio } from "@/lib/tools/audioTrimmer";
import { fileName } from "@/lib/utils";
import { Waveform } from "./Waveform";
import type { AppErrorEnvelope, JobResult, Opts } from "./types";

// Fade duration applied when the user checks "Fade in" / "Fade out".
// Per the working doc: a single fixed value rather than a configurable
// input; "reasonable, on the shorter side."
const FADE_PRESET_MS = 1000;

// Rust → TS state machine for the trimmer. Only one file at a time, so
// no "skipped" lane — a per-file failure becomes the rejected `trimAudio`
// promise and routes through the `picked.error` slot.
type ViewState =
  | { kind: "idle"; error?: AppErrorEnvelope }
  | { kind: "loading"; path: string }
  | {
      kind: "picked";
      path: string;
      source: AudioPreviewSource;
      error?: AppErrorEnvelope;
    }
  | { kind: "running"; path: string; source: AudioPreviewSource }
  | { kind: "done"; result: JobResult };

export function AudioTrimmer() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Marker + fade state lives outside ViewState so the user's choices
  // survive transitions between picked / running. Re-initialised on
  // each fresh pick (see the load `useEffect` below).
  const [startMs, setStartMs] = useState(0);
  const [endMs, setEndMs] = useState(0);
  const [fadeIn, setFadeIn] = useState(false);
  const [fadeOut, setFadeOut] = useState(false);

  const abortRef = useRef<AbortController | null>(null);
  const previewRef = useRef<PreviewPlayer | null>(null);
  const [previewing, setPreviewing] = useState(false);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  // Stop preview when leaving the picked state OR unmounting.
  useEffect(() => {
    return () => {
      previewRef.current?.stop();
      previewRef.current = null;
    };
  }, []);

  const stopPreview = () => {
    previewRef.current?.stop();
    previewRef.current = null;
    setPreviewing(false);
  };

  const pick = async () => {
    stopPreview();
    const picked = await pickAudioFile();
    if (!picked) return;
    setState({ kind: "loading", path: picked });
    try {
      await allowMediaPreview([picked]);
      const source = await loadAudioPreview(picked);
      setStartMs(0);
      setEndMs(source.durationMs);
      setFadeIn(false);
      setFadeOut(false);
      setState({ kind: "picked", path: picked, source });
    } catch (err) {
      const envelope: AppErrorEnvelope =
        typeof err === "object" && err !== null && "kind" in err
          ? (err as AppErrorEnvelope)
          : {
              kind: "ProcessingFailed",
              message:
                err instanceof Error ? err.message : "failed to load audio",
            };
      setState({ kind: "idle", error: envelope });
    }
  };

  const togglePreview = () => {
    if (state.kind !== "picked") return;
    if (previewing) {
      stopPreview();
      return;
    }
    previewRef.current = createPreviewPlayer(
      state.source.audioContext,
      state.source.audioBuffer,
      {
        startMs,
        endMs,
        fadeInMs: fadeIn ? FADE_PRESET_MS : 0,
        fadeOutMs: fadeOut ? FADE_PRESET_MS : 0,
      },
      () => {
        setPreviewing(false);
      },
    );
    setPreviewing(true);
  };

  const buildOpts = (): Opts => ({
    start_ms: startMs,
    end_ms: endMs,
    fade_in_ms: fadeIn ? FADE_PRESET_MS : 0,
    fade_out_ms: fadeOut ? FADE_PRESET_MS : 0,
  });

  // Any change to the trim window or fades invalidates the running
  // preview — its `[startMs, endMs]` window and gain envelope were
  // baked in at scheduling time. Stop it; user can hit Play again
  // with the new values. The clamps below also enforce a silent
  // minimum trim duration (start + 1 ms ≤ end ≤ source duration) so
  // the Trim button never has to disable for a zero-length window.
  const upperBound = (): number =>
    state.kind === "picked"
      ? state.source.durationMs
      : Number.POSITIVE_INFINITY;
  const onStartChange = (ms: number) => {
    stopPreview();
    setStartMs(Math.max(0, Math.min(ms, endMs - 1, upperBound() - 1)));
  };
  const onEndChange = (ms: number) => {
    stopPreview();
    setEndMs(Math.min(upperBound(), Math.max(ms, startMs + 1)));
  };
  const onRangeChange = (s: number, e: number) => {
    // Waveform sends both endpoints — clamp once, set both.
    stopPreview();
    const upper = upperBound();
    const newStart = Math.max(0, Math.min(s, upper - 1));
    const newEnd = Math.min(upper, Math.max(e, newStart + 1));
    setStartMs(newStart);
    setEndMs(newEnd);
  };
  const onFadeInToggle = (b: boolean) => {
    stopPreview();
    setFadeIn(b);
  };
  const onFadeOutToggle = (b: boolean) => {
    stopPreview();
    setFadeOut(b);
  };

  const trim = async () => {
    if (state.kind !== "picked") return;
    stopPreview();
    const controller = new AbortController();
    abortRef.current = controller;
    const running: ViewState = {
      kind: "running",
      path: state.path,
      source: state.source,
    };
    setState(running);
    try {
      const result = await trimAudio(state.path, buildOpts(), {
        signal: controller.signal,
      });
      setState({ kind: "done", result });
    } catch (err) {
      const envelope = err as AppErrorEnvelope;
      setState({
        kind: "picked",
        path: running.path,
        source: running.source,
        error: envelope,
      });
    } finally {
      abortRef.current = null;
    }
  };

  const cancel = () => abortRef.current?.abort();

  const reset = () => {
    stopPreview();
    setState({ kind: "idle" });
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Audio Trimmer</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Trim an audio file to a range. Output preserves the source format and
          lands next to the input as <code>{"{stem}_trimmed.{ext}"}</code>.
        </p>
      </header>

      {state.kind === "idle" && (
        <div className="space-y-3">
          {state.error && <ErrorAlert envelope={state.error} />}
          <Button onClick={() => void pick()}>Select audio file</Button>
        </div>
      )}

      {state.kind === "loading" && (
        <div className="text-sm text-muted-foreground">
          Decoding <span className="font-mono">{fileName(state.path)}</span>…
        </div>
      )}

      {state.kind === "picked" && (
        <PickedView
          path={state.path}
          source={state.source}
          startMs={startMs}
          endMs={endMs}
          onStartChange={onStartChange}
          onEndChange={onEndChange}
          onRangeChange={onRangeChange}
          fadeIn={fadeIn}
          fadeOut={fadeOut}
          onFadeInToggle={onFadeInToggle}
          onFadeOutToggle={onFadeOutToggle}
          previewing={previewing}
          togglePreview={togglePreview}
          onTrim={() => void trim()}
          onPickDifferent={() => void pick()}
          error={state.error}
        />
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Trimming</div>
            <div className="mt-1 font-medium font-mono">
              {fileName(state.path)}
            </div>
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
            {state.result.warnings.length > 0 && (
              <ul className="mt-2 list-disc pl-5 text-xs text-muted-foreground">
                {state.result.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            )}
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

interface PickedViewProps {
  path: string;
  source: AudioPreviewSource;
  startMs: number;
  endMs: number;
  onStartChange: (ms: number) => void;
  onEndChange: (ms: number) => void;
  onRangeChange: (startMs: number, endMs: number) => void;
  fadeIn: boolean;
  fadeOut: boolean;
  onFadeInToggle: (b: boolean) => void;
  onFadeOutToggle: (b: boolean) => void;
  previewing: boolean;
  togglePreview: () => void;
  onTrim: () => void;
  onPickDifferent: () => void;
  error?: AppErrorEnvelope;
}

function PickedView({
  path,
  source,
  startMs,
  endMs,
  onStartChange,
  onEndChange,
  onRangeChange,
  fadeIn,
  fadeOut,
  onFadeInToggle,
  onFadeOutToggle,
  previewing,
  togglePreview,
  onTrim,
  onPickDifferent,
  error,
}: PickedViewProps) {
  return (
    <div className="space-y-5">
      {error && <ErrorAlert envelope={error} />}

      <div className="text-sm">
        <div className="font-mono">{fileName(path)}</div>
        <div className="text-xs text-muted-foreground">
          Duration {formatMs(source.durationMs)}
        </div>
      </div>

      <Waveform
        peaks={source.peaks}
        durationMs={source.durationMs}
        startMs={startMs}
        endMs={endMs}
        onChange={onRangeChange}
      />

      {/* Start input | Play/Stop button | End input. Justify-between
          aligns the End block's contents to the right edge of the row. */}
      <div className="flex items-end justify-between gap-4">
        <TimeInput
          id="trim-start"
          label="Start"
          ms={startMs}
          max={source.durationMs}
          onChange={onStartChange}
        />
        <Button
          variant="outline"
          size="icon"
          onClick={togglePreview}
          aria-label={previewing ? "Stop" : "Play"}
        >
          {previewing ? <Pause /> : <Play />}
        </Button>
        <TimeInput
          id="trim-end"
          label="End"
          ms={endMs}
          max={source.durationMs}
          onChange={onEndChange}
          align="right"
        />
      </div>

      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">Fades</legend>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={fadeIn}
            onChange={(e) => onFadeInToggle(e.target.checked)}
            aria-label="Fade in"
          />
          Fade in ({FADE_PRESET_MS} ms)
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={fadeOut}
            onChange={(e) => onFadeOutToggle(e.target.checked)}
            aria-label="Fade out"
          />
          Fade out ({FADE_PRESET_MS} ms)
        </label>
      </fieldset>

      <div className="flex gap-3">
        <Button onClick={onTrim}>Trim</Button>
        <Button variant="outline" onClick={onPickDifferent}>
          Pick different file
        </Button>
      </div>
    </div>
  );
}

interface TimeInputProps {
  id: string;
  label: string;
  ms: number;
  max: number;
  onChange: (ms: number) => void;
  /// "right" right-aligns the input's text + label so the End block
  /// reads with its content flush against the row's right edge. Default
  /// is left-aligned (used by Start).
  align?: "left" | "right";
}

function TimeInput({
  id,
  label,
  ms,
  max,
  onChange,
  align = "left",
}: TimeInputProps) {
  // Render the live value as a controlled string so partial edits
  // ("00:02:") don't get reformatted mid-keystroke. Commit on blur
  // (and Enter); reformatted display lands then.
  const [text, setText] = useState(() => formatMs(ms));
  // Sync with the parent prop via a render-phase comparison rather
  // than `useEffect` — keeps `setText` out of an effect body (lint
  // forbids it) without a cascade. React converges on the next render
  // when `lastMs === ms`.
  const [lastMs, setLastMs] = useState(ms);
  if (ms !== lastMs) {
    setLastMs(ms);
    setText(formatMs(ms));
  }

  const commit = (raw: string) => {
    const parsed = parseMs(raw);
    if (parsed === null) {
      setText(formatMs(ms));
      return;
    }
    const clamped = Math.min(max, Math.max(0, parsed));
    onChange(clamped);
    setText(formatMs(clamped));
  };

  const alignClass = align === "right" ? "text-right" : "text-left";
  return (
    <div className={`space-y-1 ${align === "right" ? "text-right" : ""}`}>
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="text"
        inputMode="numeric"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit(e.currentTarget.value);
        }}
        className={`font-mono w-32 ${alignClass}`}
      />
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

/// Format a duration in milliseconds as `MM:SS.mmm`. Negative inputs
/// clamp to zero. Reused by the numeric inputs and the picked-view
/// summary; exported for tests.
export function formatMs(ms: number): string {
  const total = Math.max(0, Math.floor(ms));
  const mins = Math.floor(total / 60_000);
  const secs = Math.floor((total % 60_000) / 1000);
  const millis = total % 1000;
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

/// Parse `MM:SS` or `MM:SS.mmm` (millis 1–3 digits) into a millisecond
/// count. Returns `null` for malformed input; the caller falls back to
/// the previous valid value. Exported for tests.
export function parseMs(s: string): number | null {
  const trimmed = s.trim();
  const match = /^(\d+):(\d+)(?:\.(\d{1,3}))?$/.exec(trimmed);
  if (!match) return null;
  const mins = Number(match[1]);
  const secs = Number(match[2]);
  if (secs >= 60) return null;
  const millis = match[3] !== undefined ? Number(match[3].padEnd(3, "0")) : 0;
  return mins * 60_000 + secs * 1000 + millis;
}

// Re-export FADE_PRESET_MS so tests can assert it without re-deriving.
export { FADE_PRESET_MS };
