// Pure crop-rectangle geometry — no DOM, no React. The frame editor
// (CropFrame.tsx) and the numeric inputs both drive the rect through these
// helpers; keeping them pure makes the fiddly clamp / aspect-lock math
// unit-testable without simulating pointer drags.
//
// All coordinates are in SOURCE-image pixels (what the backend crops on),
// never display pixels. The component converts pointer deltas from display
// px to source px (÷ scale) before calling in.

import type { CropRect } from "@/lib/tools/imageCrop";

// The eight resize grips. Corners are two-letter; edges one-letter.
export type ResizeHandle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

function clamp(value: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, value));
}

// The crop frame's initial state: covering the entire image.
export function fullFrame(imgW: number, imgH: number): CropRect {
  return { x: 0, y: 0, width: imgW, height: imgH };
}

// Snap a rect fully inside [0,imgW]×[0,imgH] with a 1px minimum on each
// dimension. Coordinates are rounded to whole pixels (the crop is integer).
export function clampRect(
  rect: CropRect,
  imgW: number,
  imgH: number,
): CropRect {
  const width = clamp(Math.round(rect.width), 1, imgW);
  const height = clamp(Math.round(rect.height), 1, imgH);
  const x = clamp(Math.round(rect.x), 0, imgW - width);
  const y = clamp(Math.round(rect.y), 0, imgH - height);
  return { x, y, width, height };
}

// Translate the frame by a source-pixel delta, keeping its size and staying
// inside the image (body drag).
export function moveRect(
  start: CropRect,
  dx: number,
  dy: number,
  imgW: number,
  imgH: number,
): CropRect {
  return clampRect(
    {
      x: start.x + dx,
      y: start.y + dy,
      width: start.width,
      height: start.height,
    },
    imgW,
    imgH,
  );
}

function movesLeft(h: ResizeHandle): boolean {
  return h === "w" || h === "nw" || h === "sw";
}
function movesRight(h: ResizeHandle): boolean {
  return h === "e" || h === "ne" || h === "se";
}
function movesTop(h: ResizeHandle): boolean {
  return h === "n" || h === "nw" || h === "ne";
}
function movesBottom(h: ResizeHandle): boolean {
  return h === "s" || h === "sw" || h === "se";
}

// Resize the frame by dragging grip `handle` a source-pixel delta `(dx, dy)`
// from `start`. When `lockedRatio` is non-null the width:height ratio is held
// at that value, anchored at the corner/edge opposite the grip.
export function resizeRect(
  handle: ResizeHandle,
  start: CropRect,
  dx: number,
  dy: number,
  imgW: number,
  imgH: number,
  lockedRatio: number | null,
): CropRect {
  // Free resize: move only the grip's edges. A handle moves at most one of
  // {left,right} and one of {top,bottom}, so each moving edge clamps against
  // its FIXED partner (min 1px apart) and the image bound. Fixed edges keep
  // their already-in-bounds start values — clamping them too would let a
  // grip dragged past its partner drag the fixed edge along with it.
  let left = start.x;
  let top = start.y;
  let right = start.x + start.width;
  let bottom = start.y + start.height;
  if (movesLeft(handle)) left = clamp(start.x + dx, 0, right - 1);
  if (movesRight(handle))
    right = clamp(start.x + start.width + dx, left + 1, imgW);
  if (movesTop(handle)) top = clamp(start.y + dy, 0, bottom - 1);
  if (movesBottom(handle))
    bottom = clamp(start.y + start.height + dy, top + 1, imgH);

  const free: CropRect = {
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  };
  if (lockedRatio === null) {
    return clampRect(free, imgW, imgH);
  }
  return lockRatio(handle, start, free, lockedRatio, imgW, imgH);
}

// Re-shape `free` (the unlocked candidate) to width:height === ratio,
// anchored at the grip's opposite corner/edge, fitting inside the image.
function lockRatio(
  handle: ResizeHandle,
  start: CropRect,
  free: CropRect,
  ratio: number,
  imgW: number,
  imgH: number,
): CropRect {
  const horizontalPrimary =
    handle.length === 2 || handle === "e" || handle === "w";

  let width: number;
  let height: number;
  if (horizontalPrimary) {
    width = free.width;
    height = width / ratio;
  } else {
    height = free.height;
    width = height * ratio;
  }

  // Fit within the image while preserving the ratio.
  if (width > imgW) {
    width = imgW;
    height = width / ratio;
  }
  if (height > imgH) {
    height = imgH;
    width = height * ratio;
  }
  width = Math.max(1, width);
  height = Math.max(1, height);

  // Anchor at the fixed edge(s): if the grip moves an edge, its opposite
  // edge stays put.
  const startRight = start.x + start.width;
  const startBottom = start.y + start.height;
  const x = movesLeft(handle) ? startRight - width : start.x;
  const y = movesTop(handle) ? startBottom - height : start.y;

  return clampRect({ x, y, width, height }, imgW, imgH);
}

// Numeric-input edits. Each keeps the rect valid; the width/height setters
// also drive the partner dimension when a ratio is locked.

export function withX(
  rect: CropRect,
  x: number,
  imgW: number,
  imgH: number,
): CropRect {
  return clampRect({ ...rect, x }, imgW, imgH);
}

export function withY(
  rect: CropRect,
  y: number,
  imgW: number,
  imgH: number,
): CropRect {
  return clampRect({ ...rect, y }, imgW, imgH);
}

export function withWidth(
  rect: CropRect,
  width: number,
  lockedRatio: number | null,
  imgW: number,
  imgH: number,
): CropRect {
  const height = lockedRatio === null ? rect.height : width / lockedRatio;
  return clampRect({ ...rect, width, height }, imgW, imgH);
}

export function withHeight(
  rect: CropRect,
  height: number,
  lockedRatio: number | null,
  imgW: number,
  imgH: number,
): CropRect {
  const width = lockedRatio === null ? rect.width : height * lockedRatio;
  return clampRect({ ...rect, width, height }, imgW, imgH);
}
