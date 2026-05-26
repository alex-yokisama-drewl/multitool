import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  allowMediaPreview,
  imageAssetUrl,
  pickRasterImage,
  revealInFolder,
} from "@/lib/system";
import { cropImage } from "@/lib/tools/imageCrop";
import { fileName } from "@/lib/utils";
import { CropFrame } from "./CropFrame";
import { fullFrame, withHeight, withWidth, withX, withY } from "./cropGeometry";
import type { AppErrorEnvelope, CropRect, JobResult } from "./types";

// Rust → TS state machine. One image at a time, so no skip lane — a failed
// crop becomes the rejected `cropImage` promise and routes through the
// `picked.error` slot, preserving the picked file for a retry.
type ViewState =
  | { kind: "idle"; error?: AppErrorEnvelope }
  | { kind: "picked"; path: string; error?: AppErrorEnvelope }
  | { kind: "cropping"; path: string }
  | { kind: "done"; result: JobResult };

export function ImageCrop() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Crop geometry lives outside ViewState so it survives picked↔cropping
  // transitions. `dims` is null until the image loads; `rect` is meaningless
  // until then. The aspect lock captures `width/height` at toggle-on.
  const [dims, setDims] = useState<{ width: number; height: number } | null>(
    null,
  );
  const [rect, setRect] = useState<CropRect>({
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
  const [lockedRatio, setLockedRatio] = useState<number | null>(null);

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

  const pick = async () => {
    const picked = await pickRasterImage();
    if (!picked) return;
    try {
      await allowMediaPreview([picked]);
    } catch {
      // A failed scope grant only means the preview <img> may not render;
      // the crop itself still works. Don't block the flow on it.
    }
    setDims(null);
    setLockedRatio(null);
    setState({ kind: "picked", path: picked });
  };

  // Image finished loading → capture natural dims and reset the frame to
  // cover the whole image.
  const handleImageLoad = (width: number, height: number) => {
    setDims({ width, height });
    setRect(fullFrame(width, height));
  };

  const toggleProportional = (locked: boolean) => {
    // Capture the current ratio at the instant the lock turns on.
    setLockedRatio(locked && rect.height > 0 ? rect.width / rect.height : null);
  };

  const crop = async (path: string) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "cropping", path });
    try {
      const result = await cropImage(path, rect, { signal: controller.signal });
      setState({ kind: "done", result });
    } catch (err) {
      setState({ kind: "picked", path, error: err as AppErrorEnvelope });
    } finally {
      abortRef.current = null;
    }
  };

  const cancel = () => abortRef.current?.abort();
  const reset = () => setState({ kind: "idle" });

  const proportional = lockedRatio !== null;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Image Crop</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Crop an image to a rectangular region. Output preserves the source
          format and lands next to the input as{" "}
          <code>{"{stem}_cropped.{ext}"}</code>.
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

          <CropFrame
            imageUrl={imageAssetUrl(state.path)}
            imageWidth={dims?.width ?? 0}
            imageHeight={dims?.height ?? 0}
            rect={rect}
            onChange={setRect}
            onImageLoad={handleImageLoad}
            lockedRatio={lockedRatio}
          />

          {dims && (
            <>
              <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
                <NumberField
                  id="crop-x"
                  label="X"
                  value={rect.x}
                  onCommit={(v) =>
                    setRect((r) => withX(r, v, dims.width, dims.height))
                  }
                />
                <NumberField
                  id="crop-y"
                  label="Y"
                  value={rect.y}
                  onCommit={(v) =>
                    setRect((r) => withY(r, v, dims.width, dims.height))
                  }
                />
                <NumberField
                  id="crop-width"
                  label="Width"
                  value={rect.width}
                  onCommit={(v) =>
                    setRect((r) =>
                      withWidth(r, v, lockedRatio, dims.width, dims.height),
                    )
                  }
                />
                <NumberField
                  id="crop-height"
                  label="Height"
                  value={rect.height}
                  onCommit={(v) =>
                    setRect((r) =>
                      withHeight(r, v, lockedRatio, dims.width, dims.height),
                    )
                  }
                />
              </div>

              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={proportional}
                  onChange={(e) => toggleProportional(e.target.checked)}
                  aria-label="Lock proportions"
                />
                Lock proportions
              </label>
            </>
          )}

          <div className="flex gap-3">
            <Button onClick={() => void crop(state.path)} disabled={!dims}>
              Crop
            </Button>
            <Button variant="outline" onClick={() => void pick()}>
              Select a different image
            </Button>
          </div>
        </div>
      )}

      {state.kind === "cropping" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Cropping</div>
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
              Cropped to{" "}
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
              Crop another
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
// the parent `rect` value via a render-phase sync (no effect).
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
