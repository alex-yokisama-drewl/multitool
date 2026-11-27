import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  getRasterFormats,
  type RasterFormatDescriptor,
} from "@/lib/imageFormats";
import {
  allowMediaPreview,
  imageAssetUrl,
  pickRasterImage,
  revealInFolder,
} from "@/lib/system";
import { resizeImage } from "@/lib/tools/imageResize";
import { prefs } from "./prefs";
import { fileName } from "@/lib/utils";
import type { AppErrorEnvelope, Background, FitMode, JobResult } from "./types";

// Rust → TS state machine. One image at a time, so no skip lane — a
// failed resize becomes the rejected `resizeImage` promise and routes
// through the `picked.error` slot, preserving the picked file for a retry.
type ViewState =
  | { kind: "idle"; error?: AppErrorEnvelope }
  | { kind: "picked"; path: string; error?: AppErrorEnvelope }
  | { kind: "resizing"; path: string }
  | { kind: "done"; result: JobResult };

// Match a path's extension against the served raster descriptor set. Lives
// here (not in `@/lib/`) because nothing else needs it yet; promote if a
// second consumer shows up.
function findFormat(
  path: string,
  formats: RasterFormatDescriptor[],
): RasterFormatDescriptor | undefined {
  const lower = path.toLowerCase();
  return formats.find((f) =>
    f.extensions.some((ext) => lower.endsWith(`.${ext}`)),
  );
}

export function ImageResize() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Geometry + options live outside ViewState so the user's choices survive
  // picked↔resizing transitions. `dims` is null until the image loads;
  // width/height/lockedRatio are meaningless until then.
  const [dims, setDims] = useState<{ width: number; height: number } | null>(
    null,
  );
  const [width, setWidth] = useState<number>(0);
  const [height, setHeight] = useState<number>(0);
  const [lockedRatio, setLockedRatio] = useState<number | null>(null);
  // Only `fit` and the lock's boolean are remembered — width/height and
  // background are both derived per picked image. See ./prefs.ts.
  const [{ fit, lockAspect }, setOption] = prefs.use();
  const [background, setBackground] = useState<Background>("white");

  // Encodable-format set; gates the Transparent option per source ext.
  const [formats, setFormats] = useState<RasterFormatDescriptor[]>([]);

  const abortRef = useRef<AbortController | null>(null);
  // Tracks the last picked path for which we applied the auto bg default.
  // Prevents the apply-on-load effect from clobbering a user edit, and
  // re-applies cleanly when the user picks a different file.
  const defaultedBgForPath = useRef<string | null>(null);

  useEffect(() => {
    void getRasterFormats().then(setFormats);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void navigate("/");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [navigate]);

  const sourceFormat =
    state.kind === "picked" || state.kind === "resizing"
      ? findFormat(state.path, formats)
      : undefined;
  const sourceSupportsAlpha = sourceFormat?.supports_alpha ?? false;
  const proportional = lockedRatio !== null;
  // Whether the target W/H differs in aspect from the source. Cross-
  // multiplying integers avoids floating-point rounding (e.g. 100/50
  // ≠ 200/100 only because of f64 imprecision), so a 100×50 source
  // resized to 200×100 reports `false` exactly. Both products top out
  // at MAX_DIM² ≈ 2.7×10⁸ — well inside JS's 2⁵³ safe-integer range.
  //
  // Drives the visibility of the Fit + Background pickers: when the
  // aspect matches, Stretch / Contain / Cover collapse to the same
  // pixel-equivalent scale, so showing the radios would be noise.
  const aspectMismatch =
    !!dims &&
    width > 0 &&
    height > 0 &&
    width * dims.height !== dims.width * height;

  // Once a picked path AND the format set are both known, apply the
  // background default: Transparent when the source carries alpha
  // (PNG/WebP/TIFF) else White. Memoized per-path via a ref so a user
  // override on the radio group isn't clobbered by a re-render, and so
  // picking a new file re-anchors cleanly.
  useEffect(() => {
    const pickedPath = state.kind === "picked" ? state.path : null;
    if (!pickedPath || formats.length === 0) return;
    if (defaultedBgForPath.current === pickedPath) return;
    const fmt = findFormat(pickedPath, formats);
    setBackground(fmt?.supports_alpha ? "transparent" : "white");
    defaultedBgForPath.current = pickedPath;
  }, [state, formats]);

  const pick = async () => {
    const picked = await pickRasterImage();
    if (!picked) return;
    try {
      await allowMediaPreview([picked]);
    } catch {
      // A failed scope grant only means the preview <img> may not render;
      // the resize itself still works. Don't block the flow on it.
    }
    // Reset per-image state; handleImageLoad will populate after the
    // <img> resolves the natural dims, and the effect above will set
    // the background default once formats + path are both known.
    setDims(null);
    setLockedRatio(null);
    setState({ kind: "picked", path: picked });
  };

  // Image finished loading → capture natural dims, populate inputs, and
  // anchor the lock to the natural aspect when the remembered preference
  // has it on (it defaults on).
  const handleImageLoad = (w: number, h: number) => {
    setDims({ width: w, height: h });
    setWidth(w);
    setHeight(h);
    setLockedRatio(lockAspect && h > 0 ? w / h : null);
  };

  const toggleProportional = (locked: boolean) => {
    setOption("lockAspect", locked);
    // Capture the current W/H ratio at the instant the lock turns on —
    // matches imageCrop's pattern. If the user hasn't touched the inputs
    // yet, this naturally captures the source aspect.
    setLockedRatio(locked && height > 0 ? width / height : null);
  };

  const commitWidth = (v: number) => {
    const w = Math.max(1, Math.round(v));
    setWidth(w);
    if (lockedRatio !== null && lockedRatio > 0) {
      setHeight(Math.max(1, Math.round(w / lockedRatio)));
    }
  };

  const commitHeight = (v: number) => {
    const h = Math.max(1, Math.round(v));
    setHeight(h);
    if (lockedRatio !== null && lockedRatio > 0) {
      setWidth(Math.max(1, Math.round(h * lockedRatio)));
    }
  };

  const reset = () => {
    if (!dims) return;
    setWidth(dims.width);
    setHeight(dims.height);
    // If lock is currently ON, re-anchor to the natural aspect so the
    // lock invariant survives the reset. The checkbox state itself is
    // untouched; only the captured ratio is refreshed.
    if (lockedRatio !== null && dims.height > 0) {
      setLockedRatio(dims.width / dims.height);
    }
  };

  const resize = async (path: string) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "resizing", path });
    try {
      const result = await resizeImage(
        path,
        { width, height, fit, background },
        { signal: controller.signal },
      );
      setState({ kind: "done", result });
    } catch (err) {
      setState({ kind: "picked", path, error: err as AppErrorEnvelope });
    } finally {
      abortRef.current = null;
    }
  };

  const cancel = () => abortRef.current?.abort();
  const startOver = () => setState({ kind: "idle" });

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Image Resize</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Resize an image to a target width and height. Output preserves the
          source format and lands next to the input as{" "}
          <code>{"{stem}_resized.{ext}"}</code>.
        </p>
      </header>

      {state.kind === "idle" && (
        <div className="space-y-3">
          {state.error && <ErrorAlert envelope={state.error} />}
          <Button onClick={() => void pick()}>Select image</Button>
        </div>
      )}

      {state.kind === "picked" && (
        <div className="space-y-5">
          {state.error && <ErrorAlert envelope={state.error} />}

          <div className="text-sm">
            <div className="font-mono">{fileName(state.path)}</div>
            {dims && (
              <div className="text-xs text-muted-foreground">
                {dims.width} × {dims.height} px
              </div>
            )}
          </div>

          <img
            src={imageAssetUrl(state.path)}
            alt="Resize preview"
            draggable={false}
            onLoad={(e) =>
              handleImageLoad(
                e.currentTarget.naturalWidth,
                e.currentTarget.naturalHeight,
              )
            }
            className="max-h-72 w-auto rounded-md border border-border bg-muted object-contain"
          />

          {dims && (
            <>
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
                <NumberField
                  id="resize-width"
                  label="Width"
                  value={width}
                  onCommit={commitWidth}
                />
                <NumberField
                  id="resize-height"
                  label="Height"
                  value={height}
                  onCommit={commitHeight}
                />
                <div className="flex items-end">
                  <Button
                    type="button"
                    variant="outline"
                    onClick={reset}
                    aria-label="Reset dimensions"
                  >
                    Reset
                  </Button>
                </div>
              </div>

              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={proportional}
                  onChange={(e) => toggleProportional(e.target.checked)}
                  aria-label="Lock aspect ratio"
                />
                Lock aspect ratio
              </label>

              {aspectMismatch && (
                <fieldset className="space-y-3">
                  <legend className="text-sm font-medium">Fit</legend>
                  <RadioGroup
                    value={fit}
                    onValueChange={(value) =>
                      setOption("fit", value as FitMode)
                    }
                    className="flex flex-wrap gap-6"
                  >
                    <div className="flex items-center gap-2">
                      <RadioGroupItem id="fit-stretch" value="stretch" />
                      <Label htmlFor="fit-stretch">Stretch</Label>
                    </div>
                    <div className="flex items-center gap-2">
                      <RadioGroupItem id="fit-contain" value="contain" />
                      <Label htmlFor="fit-contain">Contain</Label>
                    </div>
                    <div className="flex items-center gap-2">
                      <RadioGroupItem id="fit-cover" value="cover" />
                      <Label htmlFor="fit-cover">Cover</Label>
                    </div>
                  </RadioGroup>
                </fieldset>
              )}

              {aspectMismatch && fit === "contain" && (
                <fieldset className="space-y-3">
                  <legend className="text-sm font-medium">Background</legend>
                  <RadioGroup
                    value={background}
                    onValueChange={(value) =>
                      setBackground(value as Background)
                    }
                    className="flex flex-wrap gap-6"
                  >
                    <div className="flex items-center gap-2">
                      <RadioGroupItem id="bg-white" value="white" />
                      <Label htmlFor="bg-white">White</Label>
                    </div>
                    <div className="flex items-center gap-2">
                      <RadioGroupItem id="bg-black" value="black" />
                      <Label htmlFor="bg-black">Black</Label>
                    </div>
                    {sourceSupportsAlpha && (
                      <div className="flex items-center gap-2">
                        <RadioGroupItem
                          id="bg-transparent"
                          value="transparent"
                        />
                        <Label htmlFor="bg-transparent">Transparent</Label>
                      </div>
                    )}
                  </RadioGroup>
                </fieldset>
              )}
            </>
          )}

          <div className="flex gap-3">
            <Button onClick={() => void resize(state.path)} disabled={!dims}>
              Resize
            </Button>
            <Button variant="outline" onClick={() => void pick()}>
              Select a different image
            </Button>
          </div>
        </div>
      )}

      {state.kind === "resizing" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Resizing</div>
            <div className="mt-1 font-mono font-medium">
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
              Resized to{" "}
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
            <Button variant="outline" onClick={startOver}>
              Resize another
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

interface NumberFieldProps {
  id: string;
  label: string;
  value: number;
  onCommit: (value: number) => void;
}

// A numeric input that edits as free text and commits a parsed integer on
// blur / Enter, so partial edits aren't reformatted mid-keystroke. Mirrors
// the parent value via a render-phase sync (no effect). Copied inline from
// `image-crop/ImageCrop.tsx`'s pattern; promote to `src/components/` when a
// third tool needs it.
function NumberField({ id, label, value, onCommit }: NumberFieldProps) {
  const [text, setText] = useState(() => String(value));
  const [lastValue, setLastValue] = useState(value);
  if (value !== lastValue) {
    setLastValue(value);
    setText(String(value));
  }

  const commit = (raw: string) => {
    const parsed = Number(raw);
    if (!Number.isFinite(parsed)) {
      setText(String(value));
      return;
    }
    onCommit(Math.round(parsed));
  };

  return (
    <div className="space-y-1">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="number"
        inputMode="numeric"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit(e.currentTarget.value);
        }}
        className="font-mono"
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
