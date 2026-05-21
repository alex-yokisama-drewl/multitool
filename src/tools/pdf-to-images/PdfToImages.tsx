import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress as ProgressBar } from "@/components/ui/progress";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { pickPdfFile, revealInFolder } from "@/lib/system";
import { convertPdfToImages } from "@/lib/tools/pdfToImages";
import type { AppErrorEnvelope, Format, JobResult, Progress } from "./types";

const DPI_MIN = 72;
const DPI_MAX = 600;
const DPI_DEFAULT = 150;

type ViewState =
  | { kind: "idle" }
  | { kind: "picked"; path: string }
  | { kind: "running"; path: string; progress?: Progress }
  | { kind: "done"; path: string; result: JobResult }
  | { kind: "error"; path: string; error: AppErrorEnvelope };

function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

export function PdfToImages() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });
  const [format, setFormat] = useState<Format>("png");
  const [dpi, setDpi] = useState<number>(DPI_DEFAULT);
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

  const choosePdf = async () => {
    const path = await pickPdfFile();
    if (path) setState({ kind: "picked", path });
  };

  const startConvert = async (path: string) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "running", path });
    try {
      const result = await convertPdfToImages(
        path,
        { format, dpi },
        {
          signal: controller.signal,
          onProgress: (progress) => {
            setState((prev) =>
              prev.kind === "running" ? { ...prev, progress } : prev,
            );
          },
        },
      );
      setState({ kind: "done", path, result });
    } catch (err) {
      const envelope = err as AppErrorEnvelope;
      setState({ kind: "error", path, error: envelope });
    } finally {
      abortRef.current = null;
    }
  };

  const cancel = () => abortRef.current?.abort();

  const reset = () => {
    setState({ kind: "idle" });
    setFormat("png");
    setDpi(DPI_DEFAULT);
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">PDF → Images</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Render each page of a PDF as a PNG or JPEG into a sibling folder.
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void choosePdf()}>Choose PDF</Button>
      )}

      {(state.kind === "picked" || state.kind === "error") && (
        <div className="space-y-5">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Input</div>
            <div className="mt-1 break-all font-medium">
              {fileName(state.path)}
            </div>
          </div>

          {state.kind === "error" && (
            <div
              role="alert"
              className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive"
            >
              <div className="font-medium">{state.error.kind}</div>
              <div className="mt-1">{state.error.message}</div>
            </div>
          )}

          <fieldset className="space-y-3">
            <legend className="text-sm font-medium">Format</legend>
            <RadioGroup
              value={format}
              onValueChange={(value) => setFormat(value as Format)}
              className="flex gap-6"
            >
              <div className="flex items-center gap-2">
                <RadioGroupItem id="format-png" value="png" />
                <Label htmlFor="format-png">PNG</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="format-jpeg" value="jpeg" />
                <Label htmlFor="format-jpeg">JPEG</Label>
              </div>
            </RadioGroup>
          </fieldset>

          <div className="space-y-2">
            <Label htmlFor="dpi">DPI</Label>
            <Input
              id="dpi"
              type="number"
              min={DPI_MIN}
              max={DPI_MAX}
              value={dpi}
              onChange={(event) => {
                const next = Number(event.target.value);
                setDpi(Number.isFinite(next) ? next : DPI_DEFAULT);
              }}
              className="w-32"
            />
            <p className="text-xs text-muted-foreground">
              {DPI_MIN}–{DPI_MAX}; defaults to {DPI_DEFAULT}.
            </p>
          </div>

          <div className="flex gap-3">
            <Button onClick={() => void startConvert(state.path)}>
              Convert
            </Button>
            <Button variant="outline" onClick={() => void choosePdf()}>
              Choose a different PDF
            </Button>
          </div>
        </div>
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Converting</div>
            <div className="mt-1 break-all font-medium">
              {fileName(state.path)}
            </div>
          </div>
          <ProgressBar
            value={
              state.progress
                ? (state.progress.page / state.progress.total) * 100
                : 0
            }
          />
          <div className="text-sm text-muted-foreground">
            {state.progress
              ? `page ${state.progress.page} / ${state.progress.total}`
              : "starting…"}
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
              Wrote {state.result.page_count}{" "}
              {state.result.page_count === 1 ? "page" : "pages"} to{" "}
              <span className="break-all font-medium">
                {state.result.output_dir}
              </span>
            </div>
          </div>
          <div className="flex gap-3">
            <Button
              onClick={() => void revealInFolder(state.result.output_dir)}
            >
              Open output folder
            </Button>
            <Button variant="outline" onClick={reset}>
              Convert another
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
