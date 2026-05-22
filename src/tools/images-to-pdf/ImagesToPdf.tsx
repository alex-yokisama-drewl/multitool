import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
  useSortable,
} from "@dnd-kit/sortable";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { JobProgress } from "@/components/JobProgress";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import {
  allowImagePreview,
  pickImageFiles,
  revealInFolder,
} from "@/lib/system";
import { convertImagesToPdf } from "@/lib/tools/imagesToPdf";
import type { AppErrorEnvelope, JobResult, PageSize, Progress } from "./types";

// View state mirrors the brief's state machine:
//   idle → staging ⇄ (add-more | remove | reorder | error) → running → done
// Error is folded back into `staging` (with the items preserved + an
// error-envelope field) so the user can retry without re-picking.
//
// Items carry a stable opaque `id` separate from `path` so duplicate paths
// — which the brief explicitly allows — don't collide as dnd-kit keys.
// The id never reaches IPC; convertImagesToPdf gets paths only.
interface StagedItem {
  id: string;
  path: string;
}
type ViewState =
  | { kind: "idle" }
  | { kind: "staging"; items: StagedItem[]; error?: AppErrorEnvelope }
  | { kind: "running"; items: StagedItem[]; progress?: Progress }
  | { kind: "done"; result: JobResult };

function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

/** Sort items by filename ascending — the brief's "Initial order on each
 * pick batch" rule. Applied on every pick (initial + add-more) so the user
 * gets a stable starting point before any manual reorder. */
function sortByFilename(items: StagedItem[]): StagedItem[] {
  return [...items].sort((a, b) =>
    fileName(a.path).localeCompare(fileName(b.path), undefined, {
      sensitivity: "base",
    }),
  );
}

function toItems(paths: string[]): StagedItem[] {
  return paths.map((path) => ({ id: crypto.randomUUID(), path }));
}

interface ThumbCardProps {
  item: StagedItem;
  onRemove: (id: string) => void;
}

function ThumbCard({ item, onRemove }: ThumbCardProps) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: item.id });

  // Inline transform serialization — avoids pulling in `@dnd-kit/utilities`
  // just for `CSS.Transform.toString`. `transform` is `{ x, y, scaleX,
  // scaleY }` from useSortable; we only need translate for a sortable grid.
  const style: React.CSSProperties = {
    transform: transform
      ? `translate3d(${transform.x}px, ${transform.y}px, 0)`
      : undefined,
    transition,
    opacity: isDragging ? 0.5 : undefined,
  };

  // The card itself is the drag handle (mouse + keyboard). The remove
  // button stops propagation so clicking × never initiates a drag.
  return (
    <li
      ref={setNodeRef}
      style={style}
      className="relative flex flex-col rounded-md border border-border bg-card"
    >
      <button
        type="button"
        aria-label={`Reorder ${fileName(item.path)}`}
        {...attributes}
        {...listeners}
        className="flex w-full flex-1 cursor-grab flex-col items-center gap-2 p-2 active:cursor-grabbing focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <img
          src={convertFileSrc(item.path)}
          alt=""
          draggable={false}
          className="h-24 w-full rounded object-contain"
        />
        <span className="line-clamp-2 break-all text-center text-xs">
          {fileName(item.path)}
        </span>
      </button>
      <button
        type="button"
        aria-label={`Remove ${fileName(item.path)}`}
        onClick={(event) => {
          event.stopPropagation();
          onRemove(item.id);
        }}
        className="absolute right-1 top-1 inline-flex h-6 w-6 items-center justify-center rounded-full border border-border bg-background text-sm leading-none shadow-sm hover:bg-accent"
      >
        ×
      </button>
    </li>
  );
}

export function ImagesToPdf() {
  const navigate = useNavigate();
  const [state, setState] = useState<ViewState>({ kind: "idle" });
  // pageSize lives outside ViewState so the user's choice survives state
  // transitions (e.g. an error returning to staging keeps the selection).
  const [pageSize, setPageSize] = useState<PageSize>("auto-fit");
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

  const sensors = useSensors(
    useSensor(PointerSensor),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const addImages = async () => {
    const picked = await pickImageFiles();
    if (!picked) return;
    // Grant per-path asset-protocol scope so `convertFileSrc(path)` in
    // ThumbCard can resolve the URL. See DECISIONS → "Asset protocol
    // scope: dynamic per-pick".
    await allowImagePreview(picked);
    setState((prev) => {
      const merged =
        prev.kind === "staging"
          ? [...prev.items, ...toItems(picked)]
          : toItems(picked);
      // Clear any prior error on a fresh pick — the user is moving forward.
      return { kind: "staging", items: sortByFilename(merged) };
    });
  };

  const removeItem = (id: string) => {
    setState((prev) => {
      if (prev.kind !== "staging") return prev;
      const next = prev.items.filter((item) => item.id !== id);
      // Empty-after-removal returns to idle per the brief — not back to
      // the dashboard, just the picker again.
      return next.length === 0
        ? { kind: "idle" }
        : { kind: "staging", items: next };
    });
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    setState((prev) => {
      if (prev.kind !== "staging") return prev;
      const from = prev.items.findIndex((item) => item.id === active.id);
      const to = prev.items.findIndex((item) => item.id === over.id);
      if (from === -1 || to === -1) return prev;
      return { kind: "staging", items: arrayMove(prev.items, from, to) };
    });
  };

  const createPdf = async (items: StagedItem[]) => {
    const controller = new AbortController();
    abortRef.current = controller;
    setState({ kind: "running", items });
    try {
      const result = await convertImagesToPdf(
        items.map((item) => item.path),
        { page_size: pageSize },
        {
          signal: controller.signal,
          onProgress: (progress) => {
            setState((prev) =>
              prev.kind === "running" ? { ...prev, progress } : prev,
            );
          },
        },
      );
      setState({ kind: "done", result });
    } catch (err) {
      // Error preserves the staging list per the brief — the user can
      // retry without re-picking. Cancellation surfaces the same way
      // (envelope kind = "Cancelled"); a future polish could suppress
      // that specific kind from the alert.
      const envelope = err as AppErrorEnvelope;
      setState({ kind: "staging", items, error: envelope });
    } finally {
      abortRef.current = null;
    }
  };

  const cancel = () => abortRef.current?.abort();

  const reset = () => {
    setState({ kind: "idle" });
    setPageSize("auto-fit");
  };

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-xl font-semibold">Images → PDF</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Assemble images into a single PDF, one image per page.
        </p>
      </header>

      {state.kind === "idle" && (
        <Button onClick={() => void addImages()}>Add images</Button>
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
            Staged ({state.items.length}). Drag to reorder; use Tab + Space for
            keyboard reorder.
          </div>
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
          >
            <SortableContext
              items={state.items.map((item) => item.id)}
              strategy={rectSortingStrategy}
            >
              <ul
                role="list"
                aria-label="Staged images"
                className="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4"
              >
                {state.items.map((item) => (
                  <ThumbCard key={item.id} item={item} onRemove={removeItem} />
                ))}
              </ul>
            </SortableContext>
          </DndContext>

          <fieldset className="space-y-3">
            <legend className="text-sm font-medium">Page size</legend>
            <RadioGroup
              value={pageSize}
              onValueChange={(value) => setPageSize(value as PageSize)}
              className="flex flex-wrap gap-6"
            >
              <div className="flex items-center gap-2">
                <RadioGroupItem id="page-auto-fit" value="auto-fit" />
                <Label htmlFor="page-auto-fit">Auto-fit (per image)</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="page-a4" value="a4" />
                <Label htmlFor="page-a4">A4</Label>
              </div>
              <div className="flex items-center gap-2">
                <RadioGroupItem id="page-letter" value="letter" />
                <Label htmlFor="page-letter">Letter</Label>
              </div>
            </RadioGroup>
          </fieldset>

          <div className="flex gap-3">
            <Button
              onClick={() => void createPdf(state.items)}
              disabled={state.items.length === 0}
            >
              Create PDF
            </Button>
            <Button variant="outline" onClick={() => void addImages()}>
              Add more images
            </Button>
          </div>
        </div>
      )}

      {state.kind === "running" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Converting</div>
            <div className="mt-1">
              {state.items.length}{" "}
              {state.items.length === 1 ? "image" : "images"}
            </div>
          </div>
          <JobProgress
            current={state.progress?.image ?? 0}
            total={state.progress?.total ?? 0}
            label="image"
            onCancel={cancel}
          />
        </div>
      )}

      {state.kind === "done" && (
        <div className="space-y-4">
          <div className="rounded-md border border-border p-3 text-sm">
            <div className="text-xs text-muted-foreground">Done</div>
            <div className="mt-1">
              Wrote PDF to{" "}
              <span className="break-all font-medium">
                {state.result.output_path}
              </span>
            </div>
          </div>
          <div className="flex gap-3">
            <Button
              onClick={() => void revealInFolder(state.result.output_path)}
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
