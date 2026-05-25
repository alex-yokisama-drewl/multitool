import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  allowMediaPreview,
  imageAssetUrl,
  pickConvertibleImages,
  revealInFolder,
} from "@/lib/system";
import { convertImageFormat } from "@/lib/tools/imageFormatConverter";
import { fileName } from "@/lib/utils";
import type {
  AlphaHandling,
  AppErrorEnvelope,
  JobResult,
  Opts,
  Progress,
  SvgRasterSize,
  TargetFormat,
} from "./types";

// View state — mirrors the brief's state machine. There is intentionally NO
// terminal "error" state: skip+continue means a job that ran at all returns
// Ok with a `skipped` list, even if every file was skipped. Only cancellation
// and orchestrator-level failures land in `error`, and from there the staged
// list is preserved so the user can retry without re-picking.
type ViewState =
  | { kind: "idle" }
  | { kind: "staging"; paths: string[]; error?: AppErrorEnvelope }
  | {
      kind: "running";
      paths: string[];
      current?: { source: string; index: number; total: number };
    }
  | { kind: "done"; paths: string[]; result: JobResult };

// Targets that can't carry alpha — JPEG and BMP. The UI only shows the
// alpha-handling selector when one of these is active; the encoder ignores
// the value for alpha-supporting targets.
const ALPHA_LESS_TARGETS: ReadonlySet<TargetFormat> = new Set<TargetFormat>([
  "jpeg",
  "bmp",
]);

const SVG_EXT_RX = /\.svg$/i;

function hasSvgInputs(paths: string[]): boolean {
  return paths.some((p) => SVG_EXT_RX.test(p));
}

export function ImageFormatConverter() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Options live outside ViewState so the user's choices survive state
  // transitions — same pattern as Images → PDF's pageSize.
  const [targetFormat, setTargetFormat] = useState<TargetFormat>("png");
  const [jpegQuality, setJpegQuality] = useState<number>(85);
  const [alphaHandling, setAlphaHandling] =
    useState<AlphaHandling>("flatten-white");
  // SVG raster size: "natural" or { "longest-edge-px": N }. We track them
  // separately so toggling the radio doesn't lose the px value the user
  // typed.
  const [svgSizeMode, setSvgSizeMode] = useState<"natural" | "longest-edge-px">(
    "longest-edge-px",
  );
  const [svgLongestEdgePx, setSvgLongestEdgePx] = useState<number>(1024);

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

  // Single-batch flow: each picker confirmation REPLACES the staged list.
  // The user picks all the files for one conversion run at once; the
  // secondary "Select different images" button is a discard-and-repick,
  // not an append. Cancelling the picker (null) leaves the current batch
  // untouched — useful if the user opened the dialog by mistake.
  const pickImages = async () => {
    const picked = await pickConvertibleImages();
    if (!picked) return;
    // Grant per-path asset-protocol scope so `convertFileSrc(path)` in
    // the staging grid can resolve the URL. See DECISIONS → "Asset
    // protocol scope: dynamic per-pick". The Rust side re-validates
    // the extension set, so a renamed file fails closed.
    await allowMediaPreview(picked);
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

  const buildOpts = (): Opts => {
    const svgRasterSize: SvgRasterSize =
      svgSizeMode === "natural"
        ? "natural"
        : { "longest-edge-px": Math.max(1, Math.min(8192, svgLongestEdgePx)) };
    return {
      target_format: targetFormat,
      jpeg_quality: Math.max(1, Math.min(100, jpegQuality)),
      alpha_handling: alphaHandling,
      svg_raster_size: svgRasterSize,
    };
  };

  const convert = async (paths: string[]) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "running", paths });
    try {
      const result = await convertImageFormat(paths, buildOpts(), {
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

  const showAlphaHandling = ALPHA_LESS_TARGETS.has(targetFormat);
  const stagedHasSvg =
    state.kind === "staging" ||
    state.kind === "running" ||
    state.kind === "done"
      ? hasSvgInputs(state.paths)
      : false;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Image Format Converter</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Convert one or more images to PNG, JPEG, WebP (lossless), BMP, or
          TIFF.
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void pickImages()}>Select images</Button>
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

          <ul
            role="list"
            aria-label="Staged images"
            className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4"
          >
            {state.paths.map((path) => (
              <li
                key={path}
                className="relative flex flex-col items-center gap-2 rounded-md border border-border bg-card p-2"
              >
                <img
                  src={imageAssetUrl(path)}
                  alt=""
                  draggable={false}
                  className="h-24 w-full rounded object-contain"
                />
                <span className="line-clamp-2 break-all text-center text-xs">
                  {fileName(path)}
                </span>
                <button
                  type="button"
                  aria-label={`Remove ${fileName(path)}`}
                  onClick={() => removePath(path)}
                  className="absolute right-1 top-1 inline-flex h-6 w-6 items-center justify-center rounded-full border border-border bg-background text-sm leading-none shadow-sm hover:bg-accent"
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
                <RadioGroupItem id="target-png" value="png" />
                <Label htmlFor="target-png">PNG</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-jpeg" value="jpeg" />
                <Label htmlFor="target-jpeg">JPEG</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-webp" value="webp" />
                <Label htmlFor="target-webp">WebP (lossless)</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-bmp" value="bmp" />
                <Label htmlFor="target-bmp">BMP</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="target-tiff" value="tiff" />
                <Label htmlFor="target-tiff">TIFF</Label>
              </div>
            </RadioGroup>
          </fieldset>

          {targetFormat === "jpeg" && (
            <div className="space-y-2">
              <Label htmlFor="jpeg-quality">JPEG quality (1–100)</Label>
              <Input
                id="jpeg-quality"
                type="number"
                min={1}
                max={100}
                value={jpegQuality}
                onChange={(event) =>
                  setJpegQuality(Number(event.target.value) || 85)
                }
                className="w-24"
              />
            </div>
          )}

          {showAlphaHandling && (
            <fieldset className="space-y-3">
              <legend className="text-sm font-medium">
                Alpha handling (target doesn&apos;t carry alpha)
              </legend>
              <RadioGroup
                value={alphaHandling}
                onValueChange={(value) =>
                  setAlphaHandling(value as AlphaHandling)
                }
                className="flex flex-wrap gap-6"
              >
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="alpha-white" value="flatten-white" />
                  <Label htmlFor="alpha-white">Flatten on white</Label>
                </div>
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="alpha-black" value="flatten-black" />
                  <Label htmlFor="alpha-black">Flatten on black</Label>
                </div>
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="alpha-preserve" value="preserve" />
                  <Label htmlFor="alpha-preserve">
                    Skip files with transparency
                  </Label>
                </div>
              </RadioGroup>
            </fieldset>
          )}

          {stagedHasSvg && (
            <fieldset className="space-y-3">
              <legend className="text-sm font-medium">SVG raster size</legend>
              <RadioGroup
                value={svgSizeMode}
                onValueChange={(value) =>
                  setSvgSizeMode(value as "natural" | "longest-edge-px")
                }
                className="flex flex-wrap items-center gap-6"
              >
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="svg-natural" value="natural" />
                  <Label htmlFor="svg-natural">Natural (from SVG)</Label>
                </div>
                <div className="flex items-center gap-2">
                  <RadioGroupItem id="svg-longest" value="longest-edge-px" />
                  <Label htmlFor="svg-longest">Longest edge (px):</Label>
                  <Input
                    id="svg-longest-px"
                    type="number"
                    min={1}
                    max={8192}
                    value={svgLongestEdgePx}
                    onChange={(event) =>
                      setSvgLongestEdgePx(Number(event.target.value) || 1024)
                    }
                    disabled={svgSizeMode !== "longest-edge-px"}
                    className="w-24"
                  />
                </div>
              </RadioGroup>
            </fieldset>
          )}

          <div className="flex gap-3">
            <Button
              onClick={() => void convert(state.paths)}
              disabled={state.paths.length === 0}
            >
              Convert
            </Button>
            <Button variant="outline" onClick={() => void pickImages()}>
              Select different images
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
                  {state.paths.length === 1 ? "image" : "images"}
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
