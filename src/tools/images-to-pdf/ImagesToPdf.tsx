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
import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { allowImagePreview, pickImageFiles } from "@/lib/system";

// View state is split into `idle` and `staging` for E2/E3. E4 will widen
// this with `running` / `done` / `error` for the convert flow.
//
// Items carry their own stable `id` so duplicate-paths (the brief allows
// repeating an image) don't collide as dnd-kit keys. The id is opaque —
// it never reaches the IPC layer; convertImagesToPdf gets `path` only.
interface StagedItem {
  id: string;
  path: string;
}
type ViewState = { kind: "idle" } | { kind: "staging"; items: StagedItem[] };

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
        <div className="space-y-4">
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
          <Button variant="outline" onClick={() => void addImages()}>
            Add more images
          </Button>
        </div>
      )}
    </div>
  );
}
