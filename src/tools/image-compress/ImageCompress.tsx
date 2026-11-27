import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { ImageThumbnail } from "@/components/ImageThumbnail";
import { Progress } from "@/components/ui/progress";
import {
  allowMediaPreview,
  fileSize,
  pickConvertibleImages,
  revealInFolder,
} from "@/lib/system";
import { compressImages } from "@/lib/tools/imageCompress";
import { prefs } from "./prefs";
import { fileName, fileStem, formatBytes } from "@/lib/utils";
import type {
  AppErrorEnvelope,
  CompressedFile,
  JobResult,
  Opts,
  Progress as JobProgress,
} from "./types";

// View state. Same shape as the format converter's: idle → staging →
// running → done, with errors stashed back on the staging state so the
// user keeps their pick on a retry.
type ViewState =
  | { kind: "idle"; error?: AppErrorEnvelope }
  | {
      kind: "staging";
      entries: StagedEntry[];
      error?: AppErrorEnvelope;
    }
  | {
      kind: "running";
      entries: StagedEntry[];
      processed: number;
      total: number;
      current?: { source: string; index: number };
    }
  | { kind: "done"; entries: StagedEntry[]; result: JobResult };

interface StagedEntry {
  path: string;
  /** Source bytes from `stat`. `null` until the metadata fetch resolves. */
  sourceBytes: number | null;
}

// Slider granularity: 1000 steps spans [0, 1] finely enough that the
// quality-rounded backend value moves at most ~1 step per UI drag of a
// few pixels.
const SLIDER_STEPS = 1000;
// 0.8 is the "save for web" sweet spot — visibly smaller than the source
// for most JPEGs while still indistinguishable at typical viewing sizes.
// Higher than the video-compress default (0.5) because JPEG/WebP at
// midpoint is more aggressive than CRF-50 on video.

// Mirrors the Rust strategy in `image_compress::convert::strategy_for`.
// JPEG inputs preserve their source-extension spelling (`.jpg` stays
// `.jpg`, `.jpeg` stays `.jpeg`); every other accepted input goes to
// `.webp` under the per-format strategy. Keep this aligned with the Rust
// side so the predicted-output filename in staging matches what the
// orchestrator actually writes.
function sourceExtension(sourcePath: string): string {
  const name = fileName(sourcePath);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function predictedOutputExtension(sourcePath: string): string {
  const ext = sourceExtension(sourcePath);
  if (ext === "jpg" || ext === "jpeg") return ext;
  return "webp";
}

function predictedOutputName(sourcePath: string): string {
  return `${fileStem(sourcePath)}_compressed.${predictedOutputExtension(sourcePath)}`;
}

// Which source extensions get redirected to WebP under the per-source
// strategy. Keep aligned with `strategy_for` in the Rust side. Used to
// drive the warning banner so the conversion is surfaced explicitly
// instead of buried in the header copy.
const REDIRECT_TO_WEBP = new Set(["png", "bmp", "tif", "tiff"]);

function redirectCountsByExt(entries: StagedEntry[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const entry of entries) {
    const ext = sourceExtension(entry.path);
    if (REDIRECT_TO_WEBP.has(ext)) {
      counts.set(ext, (counts.get(ext) ?? 0) + 1);
    }
  }
  return counts;
}

function toEnvelope(err: unknown, fallback: string): AppErrorEnvelope {
  return typeof err === "object" && err !== null && "kind" in err
    ? (err as AppErrorEnvelope)
    : {
        kind: "ProcessingFailed",
        message: err instanceof Error ? err.message : fallback,
      };
}

async function statBytes(path: string): Promise<number | null> {
  // The backend's `file_size` command beats pulling in `tauri-plugin-fs`
  // for one lookup. A failure here only loses the staging-row size
  // display — never block the user from running the compress, since the
  // orchestrator surfaces the actual sizes per-file in the done view.
  try {
    return await fileSize(path);
  } catch {
    return null;
  }
}

export function ImageCompress() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });

  // Quality lives outside ViewState so it survives staging ↔ running
  // transitions, and in prefs so they survive the app closing.
  const [{ quality }, setOption] = prefs.use();
  const setQuality = (next: number) => {
    setOption("quality", next);
  };

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

  // Single-batch flow: each picker confirmation REPLACES the staged list,
  // same shape as the format converter. Cancelling the picker leaves the
  // current batch untouched.
  const pickImages = async () => {
    const picked = await pickConvertibleImages();
    if (!picked) return;
    // Grant per-path asset-protocol scope so the thumbnail tiles can
    // resolve via `convertFileSrc`. Same dynamic-per-pick policy as the
    // format converter (DECISIONS → "Asset protocol scope").
    await allowMediaPreview(picked);
    const entries: StagedEntry[] = picked.map((path) => ({
      path,
      sourceBytes: null,
    }));
    setState({ kind: "staging", entries });
    // Backfill sizes as the stat() calls return. We don't gate the
    // staging UI on this — the row reads "—" until the size lands.
    void Promise.all(
      picked.map(async (path, index) => {
        const size = await statBytes(path);
        setState((prev) => {
          if (prev.kind !== "staging") return prev;
          const next = prev.entries.slice();
          const current = next[index];
          if (current?.path === path) {
            next[index] = { ...current, sourceBytes: size };
          }
          return { ...prev, entries: next };
        });
      }),
    );
  };

  const removePath = (path: string) => {
    setState((prev) => {
      if (prev.kind !== "staging") return prev;
      const next = prev.entries.filter((e) => e.path !== path);
      return next.length === 0
        ? { kind: "idle" }
        : { kind: "staging", entries: next };
    });
  };

  const compress = async (entries: StagedEntry[]) => {
    const controller = new AbortController();
    abortRef.current = controller;
    const paths = entries.map((e) => e.path);
    setState({
      kind: "running",
      entries,
      processed: 0,
      total: paths.length,
    });
    try {
      const result = await compressImages(paths, { quality } satisfies Opts, {
        signal: controller.signal,
        onProgress: (progress: JobProgress) => {
          setState((prev) =>
            prev.kind === "running" ? applyProgress(prev, progress) : prev,
          );
        },
      });
      setState({ kind: "done", entries, result });
    } catch (err) {
      setState({
        kind: "staging",
        entries,
        error: toEnvelope(err, "compression failed"),
      });
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
        <h1 className="text-xl font-semibold">Image Compress</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Re-encode images at a chosen quality to produce smaller files. JPEG
          stays JPEG; PNG, BMP, TIFF, and WebP land as <code>.webp</code>.
          Output lands next to each input as{" "}
          <code>{"{stem}_compressed.{ext}"}</code>.
        </p>
      </header>

      {state.kind === "idle" && (
        <div className="space-y-3">
          {state.error && <ErrorAlert envelope={state.error} />}
          <Button onClick={() => void pickImages()}>Select image files</Button>
        </div>
      )}

      {state.kind === "staging" && (
        <div className="space-y-5">
          {state.error && <ErrorAlert envelope={state.error} />}

          <div className="text-xs text-muted-foreground">
            Staged ({state.entries.length}). Output lands next to each input.
          </div>

          <FormatRedirectBanner entries={state.entries} />

          <ul
            role="list"
            aria-label="Staged images"
            className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4"
          >
            {state.entries.map((entry) => (
              <ImageThumbnail
                key={entry.path}
                path={entry.path}
                onRemove={removePath}
                subtitle={<ThumbnailMeta entry={entry} />}
              />
            ))}
          </ul>

          <QualitySlider value={quality} onChange={setQuality} />

          <div className="flex gap-3">
            <Button
              onClick={() => void compress(state.entries)}
              disabled={state.entries.length === 0}
            >
              Compress
            </Button>
            <Button variant="outline" onClick={() => void pickImages()}>
              Pick different files
            </Button>
          </div>
        </div>
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Compressing</div>
            <div className="mt-1">
              {state.current ? (
                <>
                  {state.current.index + 1} / {state.total} —{" "}
                  <span className="font-mono font-medium">
                    {fileName(state.current.source)}
                  </span>
                </>
              ) : (
                <>
                  {state.total} {state.total === 1 ? "image" : "images"}
                </>
              )}
            </div>
            <Progress
              className="mt-3"
              value={Math.round(
                (state.processed / Math.max(1, state.total)) * 100,
              )}
              aria-label={`Compress progress: ${String(state.processed)} of ${String(state.total)}`}
            />
          </div>
          <Button variant="outline" onClick={cancel}>
            Cancel
          </Button>
        </div>
      )}

      {state.kind === "done" && (
        <DoneView result={state.result} onReset={reset} />
      )}
    </div>
  );
}

interface FormatRedirectBannerProps {
  entries: StagedEntry[];
}

// Reactive banner: shows up when any picked file's source format would
// be redirected to WebP under the per-source strategy (PNG / BMP / TIFF).
// Hidden when the batch is JPEG-only and/or WebP-only — those preserve
// their source format and the header copy already covers it. Kept
// non-destructive (amber, not red) since it's expected behaviour, not an
// error.
function FormatRedirectBanner({ entries }: FormatRedirectBannerProps) {
  const counts = redirectCountsByExt(entries);
  if (counts.size === 0) return null;
  const total = Array.from(counts.values()).reduce((a, b) => a + b, 0);
  // Stable display order: PNG, BMP, TIF/TIFF — matches the strategy table.
  const order = ["png", "bmp", "tif", "tiff"];
  const parts = order
    .filter((ext) => counts.has(ext))
    .map((ext) => `${ext.toUpperCase()} (${String(counts.get(ext) ?? 0)})`);
  return (
    <div
      role="status"
      className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-sm text-amber-900 dark:text-amber-200"
    >
      <div className="font-medium">
        {total} file{total === 1 ? "" : "s"} will be saved as WebP
      </div>
      <div className="mt-1 text-xs">
        {parts.join(", ")}. These source formats can&apos;t carry the
        compression themselves; JPEG inputs stay JPEG.
      </div>
    </div>
  );
}

interface ThumbnailMetaProps {
  entry: StagedEntry;
}

// Two-line meta block rendered under the filename in each staging tile:
// source size (resolved async via the backend `file_size` command) and
// the predicted output filename (per the source-strategy table). Falls
// back to "—" for size until the stat resolves; the row stays usable.
function ThumbnailMeta({ entry }: ThumbnailMetaProps) {
  return (
    <>
      <div>
        {entry.sourceBytes !== null ? formatBytes(entry.sourceBytes) : "—"}
      </div>
      <div className="truncate">→ {predictedOutputName(entry.path)}</div>
    </>
  );
}

interface QualitySliderProps {
  value: number;
  onChange: (next: number) => void;
}

function QualitySlider({ value, onChange }: QualitySliderProps) {
  return (
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
          value={Math.round(value * SLIDER_STEPS)}
          onChange={(e) => onChange(Number(e.target.value) / SLIDER_STEPS)}
          className="flex-1 accent-primary"
        />
        <span className="whitespace-nowrap text-xs text-muted-foreground">
          Higher quality
        </span>
      </div>
    </div>
  );
}

interface DoneViewProps {
  result: JobResult;
  onReset: () => void;
}

function DoneView({ result, onReset }: DoneViewProps) {
  return (
    <div className="space-y-4">
      <div className="rounded-md border border-border p-3 text-sm">
        <div className="text-xs text-muted-foreground">Done</div>
        <div className="mt-1">
          {result.success_count} compressed
          {result.skip_count > 0
            ? `, ${String(result.skip_count)} skipped`
            : ""}
          .
        </div>
      </div>

      {result.outputs.length > 0 && (
        <ul
          role="list"
          aria-label="Compressed files"
          className="divide-y divide-border rounded-md border border-border"
        >
          {result.outputs.map((f) => (
            <li key={f.output} className="p-3 text-sm">
              <SizeDeltaRow file={f} />
            </li>
          ))}
        </ul>
      )}

      {result.skipped.length > 0 && (
        <details className="rounded-md border border-border p-3 text-sm">
          <summary className="cursor-pointer text-sm font-medium">
            Skipped files ({result.skipped.length})
          </summary>
          <ul className="mt-2 space-y-2">
            {result.skipped.map((s) => (
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
        {result.first_output_path !== null && (
          <Button
            onClick={() => void revealInFolder(result.first_output_path ?? "")}
          >
            Open output folder
          </Button>
        )}
        <Button variant="outline" onClick={onReset}>
          Compress another batch
        </Button>
      </div>
    </div>
  );
}

interface SizeDeltaRowProps {
  file: CompressedFile;
}

function SizeDeltaRow({ file }: SizeDeltaRowProps) {
  // Percent delta as a signed integer — negative means smaller (the
  // happy case), positive means the re-encode bloated the file
  // (re-encoding a low-quality JPEG at the high end of the slider can
  // do this). Zero-source-bytes is unreachable in practice but guards
  // the divide.
  const delta =
    file.original_bytes > 0
      ? Math.round(
          ((file.compressed_bytes - file.original_bytes) /
            file.original_bytes) *
            100,
        )
      : 0;
  const sign = delta > 0 ? "+" : "";
  const deltaClass =
    delta < 0
      ? "text-emerald-600 dark:text-emerald-400"
      : delta > 0
        ? "text-amber-600 dark:text-amber-400"
        : "text-muted-foreground";
  return (
    <>
      <div className="flex items-baseline gap-2">
        <span className="truncate font-mono">{fileName(file.source)}</span>
        <span className="text-muted-foreground">→</span>
        <span className="truncate font-mono">{fileName(file.output)}</span>
      </div>
      <div className="mt-1 text-xs">
        <span className="text-muted-foreground">
          {formatBytes(file.original_bytes)} →{" "}
          {formatBytes(file.compressed_bytes)}
        </span>{" "}
        <span className={deltaClass}>
          ({sign}
          {delta}%)
        </span>
      </div>
    </>
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

// Reduce a Progress event into the running ViewState. Pulled out so the
// state setter callback stays simple and the test for it has a target.
function applyProgress(
  prev: Extract<ViewState, { kind: "running" }>,
  progress: JobProgress,
): Extract<ViewState, { kind: "running" }> {
  switch (progress.kind) {
    case "started":
      return {
        ...prev,
        current: { source: progress.source, index: progress.index },
      };
    case "succeeded":
    case "skipped":
      return { ...prev, processed: prev.processed + 1 };
  }
}
