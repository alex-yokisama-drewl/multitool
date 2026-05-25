import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { pickConvertibleAudio, revealInFolder } from "@/lib/system";
import { convertAudioFormat } from "@/lib/tools/audioFormatConverter";
import { fileName } from "@/lib/utils";
import type {
  AppErrorEnvelope,
  ChannelMode,
  JobResult,
  Opts,
  Progress,
  TargetFormat,
  WavBitDepth,
} from "./types";

// State machine mirrors ImageFormatConverter exactly. Skip+continue
// means a job that ran at all returns Ok with a `skipped` list; only
// cancellation and orchestrator-level errors land in `staging.error`.
type ViewState =
  | { kind: "idle" }
  | { kind: "staging"; paths: string[]; error?: AppErrorEnvelope }
  | {
      kind: "running";
      paths: string[];
      current?: { source: string; index: number; total: number };
    }
  | { kind: "done"; paths: string[]; result: JobResult };

const MP3_BITRATE_MIN = 96;
const MP3_BITRATE_MAX = 320;
const VORBIS_QUALITY_MIN = -1;
const VORBIS_QUALITY_MAX = 10;

export function AudioFormatConverter() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Options live outside ViewState so the user's choices survive state
  // transitions — same pattern as ImageFormatConverter.
  const [targetFormat, setTargetFormat] = useState<TargetFormat>("mp3");
  const [mp3BitrateKbps, setMp3BitrateKbps] = useState<number>(192);
  const [vorbisQuality, setVorbisQuality] = useState<number>(5);
  const [wavBitDepth, setWavBitDepth] = useState<WavBitDepth>("bit16");
  const [channels, setChannels] = useState<ChannelMode>("source");

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

  // Each picker confirmation REPLACES the staged list (per the brief —
  // "Allow batch upload, but don't append to batch, replace instead").
  // Cancelling the picker (null) leaves the current batch untouched.
  const pickAudio = async () => {
    const picked = await pickConvertibleAudio();
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

  const buildOpts = (): Opts => ({
    target_format: targetFormat,
    mp3_bitrate_kbps: Math.max(
      MP3_BITRATE_MIN,
      Math.min(MP3_BITRATE_MAX, mp3BitrateKbps),
    ),
    vorbis_quality: Math.max(
      VORBIS_QUALITY_MIN,
      Math.min(VORBIS_QUALITY_MAX, vorbisQuality),
    ),
    // No user-facing knob yet (see Opts comment on Rust side); pass through
    // the default so the wire shape is stable.
    flac_compression_level: 5,
    wav_bit_depth: wavBitDepth,
    channels,
  });

  const convert = async (paths: string[]) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "running", paths });
    try {
      const result = await convertAudioFormat(paths, buildOpts(), {
        signal: controller.signal,
        onProgress: (progress: Progress) => {
          if (progress.kind === "started") {
            setState((prev) =>
              prev.kind === "running"
                ? {
                    ...prev,
                    current: {
                      source: progress.source,
                      index: progress.index,
                      total: progress.total,
                    },
                  }
                : prev,
            );
          }
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
        <h1 className="text-xl font-semibold">Audio Format Converter</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Convert one or more audio files to WAV, FLAC, MP3, or OGG Vorbis.
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void pickAudio()}>Select audio files</Button>
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
            Staged ({state.paths.length}). Output lands next to each input.
          </div>

          <ul role="list" aria-label="Staged audio files" className="space-y-1">
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
                <RadioGroupItem id="target-mp3" value="mp3" />
                <Label htmlFor="target-mp3">MP3</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-ogg" value="ogg" />
                <Label htmlFor="target-ogg">OGG Vorbis</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-flac" value="flac" />
                <Label htmlFor="target-flac">FLAC</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-wav" value="wav" />
                <Label htmlFor="target-wav">WAV</Label>
              </div>
            </RadioGroup>
          </fieldset>

          {targetFormat === "mp3" && (
            <div className="space-y-2">
              <Label htmlFor="mp3-bitrate">
                MP3 bitrate (kbps, {MP3_BITRATE_MIN}–{MP3_BITRATE_MAX})
              </Label>
              <Input
                id="mp3-bitrate"
                type="number"
                min={MP3_BITRATE_MIN}
                max={MP3_BITRATE_MAX}
                value={mp3BitrateKbps}
                onChange={(event) =>
                  setMp3BitrateKbps(Number(event.target.value) || 192)
                }
                className="w-24"
              />
            </div>
          )}

          {targetFormat === "ogg" && (
            <div className="space-y-2">
              <Label htmlFor="ogg-quality">
                OGG quality ({VORBIS_QUALITY_MIN} to {VORBIS_QUALITY_MAX}, Xiph
                scale)
              </Label>
              <Input
                id="ogg-quality"
                type="number"
                step="0.1"
                min={VORBIS_QUALITY_MIN}
                max={VORBIS_QUALITY_MAX}
                value={vorbisQuality}
                onChange={(event) =>
                  setVorbisQuality(Number(event.target.value) || 5)
                }
                className="w-24"
              />
            </div>
          )}

          {targetFormat === "wav" && (
            <fieldset className="space-y-3">
              <legend className="text-sm font-medium">WAV bit depth</legend>
              <RadioGroup
                value={wavBitDepth}
                onValueChange={(value) => setWavBitDepth(value as WavBitDepth)}
                className="flex flex-wrap gap-6"
              >
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="wav-16" value="bit16" />
                  <Label htmlFor="wav-16">16-bit PCM</Label>
                </div>
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="wav-24" value="bit24" />
                  <Label htmlFor="wav-24">24-bit PCM</Label>
                </div>
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="wav-32f" value="bit32f" />
                  <Label htmlFor="wav-32f">32-bit float</Label>
                </div>
              </RadioGroup>
            </fieldset>
          )}

          <fieldset className="space-y-3">
            <legend className="text-sm font-medium">Channels</legend>
            <RadioGroup
              value={channels}
              onValueChange={(value) => setChannels(value as ChannelMode)}
              className="flex flex-wrap gap-6"
            >
              <div className="flex items-center gap-2">
                <RadioGroupItem id="ch-source" value="source" />
                <Label htmlFor="ch-source">Source (downmix &gt; 2 ch)</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="ch-mono" value="mono" />
                <Label htmlFor="ch-mono">Mono</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="ch-stereo" value="stereo" />
                <Label htmlFor="ch-stereo">Stereo</Label>
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
            <Button variant="outline" onClick={() => void pickAudio()}>
              Select different audio
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
