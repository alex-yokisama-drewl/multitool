import { useEffect, useRef, useState } from "react";
import type { CropRect } from "@/lib/tools/imageCrop";
import { moveRect, resizeRect, type ResizeHandle } from "./cropGeometry";

interface CropFrameProps {
  imageUrl: string;
  // Natural (source) pixel dimensions. `0` until the image has loaded — the
  // overlay frame is withheld until then.
  imageWidth: number;
  imageHeight: number;
  rect: CropRect;
  onChange: (rect: CropRect) => void;
  onImageLoad: (width: number, height: number) => void;
  lockedRatio: number | null;
}

// Static layout of the eight resize grips: where each sits on the frame's
// edge/corner and which resize cursor it shows.
const HANDLES: {
  handle: ResizeHandle;
  label: string;
  position: string;
  cursor: string;
}[] = [
  {
    handle: "nw",
    label: "top-left",
    position: "left-0 top-0",
    cursor: "cursor-nwse-resize",
  },
  {
    handle: "n",
    label: "top",
    position: "left-1/2 top-0",
    cursor: "cursor-ns-resize",
  },
  {
    handle: "ne",
    label: "top-right",
    position: "left-full top-0",
    cursor: "cursor-nesw-resize",
  },
  {
    handle: "e",
    label: "right",
    position: "left-full top-1/2",
    cursor: "cursor-ew-resize",
  },
  {
    handle: "se",
    label: "bottom-right",
    position: "left-full top-full",
    cursor: "cursor-nwse-resize",
  },
  {
    handle: "s",
    label: "bottom",
    position: "left-1/2 top-full",
    cursor: "cursor-ns-resize",
  },
  {
    handle: "sw",
    label: "bottom-left",
    position: "left-0 top-full",
    cursor: "cursor-nesw-resize",
  },
  {
    handle: "w",
    label: "left",
    position: "left-0 top-1/2",
    cursor: "cursor-ew-resize",
  },
];

interface DragState {
  handle: ResizeHandle | "move";
  pointerX: number;
  pointerY: number;
  startRect: CropRect;
}

// Image preview with a draggable rectangular crop frame overlaid. Corners and
// edges resize the frame; the body translates it. All geometry is delegated
// to `cropGeometry` (pure + unit-tested); this component only maps pointer
// deltas from display px to source px (÷ scale) and reports the new rect.
export function CropFrame({
  imageUrl,
  imageWidth,
  imageHeight,
  rect,
  onChange,
  onImageLoad,
  lockedRatio,
}: CropFrameProps) {
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const [displayWidth, setDisplayWidth] = useState(0);
  const [dragging, setDragging] = useState(false);

  // Track the rendered image width so we can convert between display and
  // source pixels. The image is laid out `w-full`, so the wrapper width is
  // the displayed image width.
  useEffect(() => {
    const wrapper = wrapperRef.current;
    if (!wrapper) return undefined;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setDisplayWidth(entry.contentRect.width);
    });
    observer.observe(wrapper);
    return () => {
      observer.disconnect();
    };
  }, []);

  const ready = imageWidth > 0 && imageHeight > 0;
  const scale = ready && displayWidth > 0 ? displayWidth / imageWidth : 0;

  useEffect(() => {
    if (!dragging) return undefined;
    const onMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag || scale <= 0) return;
      const dx = (event.clientX - drag.pointerX) / scale;
      const dy = (event.clientY - drag.pointerY) / scale;
      const next =
        drag.handle === "move"
          ? moveRect(drag.startRect, dx, dy, imageWidth, imageHeight)
          : resizeRect(
              drag.handle,
              drag.startRect,
              dx,
              dy,
              imageWidth,
              imageHeight,
              lockedRatio,
            );
      onChange(next);
    };
    const onUp = () => {
      dragRef.current = null;
      setDragging(false);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [dragging, scale, imageWidth, imageHeight, lockedRatio, onChange]);

  const beginDrag = (
    handle: ResizeHandle | "move",
    event: React.PointerEvent,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      handle,
      pointerX: event.clientX,
      pointerY: event.clientY,
      startRect: rect,
    };
    setDragging(true);
  };

  const frameStyle = {
    left: rect.x * scale,
    top: rect.y * scale,
    width: rect.width * scale,
    height: rect.height * scale,
  };

  return (
    <div ref={wrapperRef} className="relative w-full select-none">
      <img
        src={imageUrl}
        alt="Crop preview"
        draggable={false}
        onLoad={(e) =>
          onImageLoad(
            e.currentTarget.naturalWidth,
            e.currentTarget.naturalHeight,
          )
        }
        className="block w-full rounded-md border border-border bg-card"
      />
      {ready && (
        <div
          role="group"
          aria-label="Crop frame"
          onPointerDown={(e) => beginDrag("move", e)}
          className="absolute cursor-move border-2 border-primary bg-primary/10 box-border"
          style={frameStyle}
        >
          {HANDLES.map(({ handle, label, position, cursor }) => (
            <div
              key={handle}
              role="slider"
              aria-label={`Crop ${label}`}
              data-handle={handle}
              onPointerDown={(e) => beginDrag(handle, e)}
              className={`absolute h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-sm border border-primary bg-background ${position} ${cursor}`}
            />
          ))}
        </div>
      )}
    </div>
  );
}
