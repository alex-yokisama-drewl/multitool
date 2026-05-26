import { describe, expect, it } from "vitest";
import {
  clampRect,
  fullFrame,
  moveRect,
  resizeRect,
  withHeight,
  withWidth,
  withX,
  withY,
} from "./cropGeometry";

const IMG_W = 200;
const IMG_H = 100;

describe("cropGeometry", () => {
  it("fullFrame covers the whole image", () => {
    expect(fullFrame(IMG_W, IMG_H)).toEqual({
      x: 0,
      y: 0,
      width: IMG_W,
      height: IMG_H,
    });
  });

  describe("clampRect", () => {
    it("rounds and keeps an in-bounds rect", () => {
      expect(
        clampRect({ x: 10.4, y: 5.6, width: 30.2, height: 20.9 }, IMG_W, IMG_H),
      ).toEqual({
        x: 10,
        y: 6,
        width: 30,
        height: 21,
      });
    });

    it("pulls an oversized rect back inside the image", () => {
      expect(
        clampRect({ x: 180, y: 90, width: 50, height: 50 }, IMG_W, IMG_H),
      ).toEqual({
        // width clamped to 200 max then x pinned so x+width ≤ 200.
        x: 150,
        y: 50,
        width: 50,
        height: 50,
      });
    });

    it("enforces a 1px minimum on each dimension", () => {
      expect(
        clampRect({ x: 10, y: 10, width: 0, height: 0 }, IMG_W, IMG_H),
      ).toEqual({
        x: 10,
        y: 10,
        width: 1,
        height: 1,
      });
    });
  });

  describe("moveRect", () => {
    it("translates the frame, preserving size", () => {
      const r = moveRect(
        { x: 10, y: 10, width: 40, height: 30 },
        5,
        -4,
        IMG_W,
        IMG_H,
      );
      expect(r).toEqual({ x: 15, y: 6, width: 40, height: 30 });
    });

    it("clamps the frame against the image edge without shrinking", () => {
      const r = moveRect(
        { x: 180, y: 80, width: 40, height: 30 },
        100,
        100,
        IMG_W,
        IMG_H,
      );
      expect(r).toEqual({ x: 160, y: 70, width: 40, height: 30 });
    });
  });

  describe("resizeRect (free)", () => {
    it("east grip grows width, left edge fixed", () => {
      const r = resizeRect(
        "e",
        { x: 20, y: 20, width: 40, height: 30 },
        10,
        0,
        IMG_W,
        IMG_H,
        null,
      );
      expect(r).toEqual({ x: 20, y: 20, width: 50, height: 30 });
    });

    it("west grip moves the left edge, right edge fixed", () => {
      const r = resizeRect(
        "w",
        { x: 20, y: 20, width: 40, height: 30 },
        10,
        0,
        IMG_W,
        IMG_H,
        null,
      );
      // left 20→30, right stays 60 → width 30.
      expect(r).toEqual({ x: 30, y: 20, width: 30, height: 30 });
    });

    it("se corner grows both dimensions", () => {
      const r = resizeRect(
        "se",
        { x: 10, y: 10, width: 40, height: 30 },
        20,
        10,
        IMG_W,
        IMG_H,
        null,
      );
      expect(r).toEqual({ x: 10, y: 10, width: 60, height: 40 });
    });

    it("prevents inversion when a grip is dragged past its fixed edge", () => {
      // Drag the east edge far left, past the left edge → clamps to 1px.
      const r = resizeRect(
        "e",
        { x: 20, y: 20, width: 40, height: 30 },
        -100,
        0,
        IMG_W,
        IMG_H,
        null,
      );
      expect(r.width).toBe(1);
      expect(r.x).toBe(20);
    });

    it("clamps a grip drag at the image edge", () => {
      const r = resizeRect(
        "e",
        { x: 20, y: 20, width: 40, height: 30 },
        1000,
        0,
        IMG_W,
        IMG_H,
        null,
      );
      expect(r).toEqual({ x: 20, y: 20, width: IMG_W - 20, height: 30 });
    });
  });

  describe("resizeRect (ratio-locked)", () => {
    it("se corner keeps the locked ratio anchored at top-left", () => {
      // start 40×20, ratio 2:1. Drag se +20 in x → width 60, height 30.
      const r = resizeRect(
        "se",
        { x: 10, y: 10, width: 40, height: 20 },
        20,
        5,
        IMG_W,
        IMG_H,
        2,
      );
      expect(r).toEqual({ x: 10, y: 10, width: 60, height: 30 });
    });

    it("nw corner keeps the ratio anchored at bottom-right", () => {
      // start at (10,10) 40×20, right=50 bottom=30, ratio 2. Dragging nw
      // left by -20 would want width 60, but the left edge clamps at the
      // image boundary (x=0) first, capping width at 50 → height 25,
      // anchored at the fixed bottom-right corner (50,30): x=0, y=30-25=5.
      const r = resizeRect(
        "nw",
        { x: 10, y: 10, width: 40, height: 20 },
        -20,
        0,
        IMG_W,
        IMG_H,
        2,
      );
      expect(r).toEqual({ x: 0, y: 5, width: 50, height: 25 });
    });

    it("east edge under lock derives height from width", () => {
      // ratio 2:1, east +20 → width 60 → height 30, anchored top-left.
      const r = resizeRect(
        "e",
        { x: 10, y: 10, width: 40, height: 20 },
        20,
        0,
        IMG_W,
        IMG_H,
        2,
      );
      expect(r).toEqual({ x: 10, y: 10, width: 60, height: 30 });
    });

    it("south edge under lock derives width from height", () => {
      // ratio 2:1, south +10 → height 30 → width 60, anchored top-left.
      const r = resizeRect(
        "s",
        { x: 10, y: 10, width: 40, height: 20 },
        10,
        10,
        IMG_W,
        IMG_H,
        2,
      );
      expect(r).toEqual({ x: 10, y: 10, width: 60, height: 30 });
    });

    it("fits a locked resize that would overflow the image", () => {
      // ratio 2:1 near the right edge; a big east drag should cap width to
      // the image and keep the ratio (height = width/2).
      const r = resizeRect(
        "e",
        { x: 0, y: 0, width: 40, height: 20 },
        1000,
        0,
        IMG_W,
        IMG_H,
        2,
      );
      expect(r.width).toBe(IMG_W);
      expect(r.height).toBe(IMG_W / 2);
    });
  });

  describe("numeric setters", () => {
    it("withX / withY clamp into the image", () => {
      const start = { x: 10, y: 10, width: 40, height: 30 };
      expect(withX(start, -5, IMG_W, IMG_H).x).toBe(0);
      expect(withY(start, 999, IMG_W, IMG_H).y).toBe(IMG_H - 30);
    });

    it("withWidth without lock changes only width", () => {
      const r = withWidth(
        { x: 10, y: 10, width: 40, height: 30 },
        80,
        null,
        IMG_W,
        IMG_H,
      );
      expect(r).toEqual({ x: 10, y: 10, width: 80, height: 30 });
    });

    it("withWidth with a locked ratio drives height", () => {
      const r = withWidth(
        { x: 10, y: 10, width: 40, height: 20 },
        80,
        2,
        IMG_W,
        IMG_H,
      );
      expect(r).toEqual({ x: 10, y: 10, width: 80, height: 40 });
    });

    it("withHeight with a locked ratio drives width", () => {
      const r = withHeight(
        { x: 10, y: 10, width: 40, height: 20 },
        40,
        2,
        IMG_W,
        IMG_H,
      );
      expect(r).toEqual({ x: 10, y: 10, width: 80, height: 40 });
    });
  });
});
