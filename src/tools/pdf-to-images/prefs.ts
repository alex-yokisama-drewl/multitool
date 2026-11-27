// Remembered options for this tool. Lives in the tool's own folder so the
// registry contract holds — nothing shared changes to give a tool memory.
//
// Both options are workflow preferences ("I always render at 300 DPI as
// JPEG"), not properties of the file in front of the user, so both stick.

import { definePrefs, finiteNumber, oneOf } from "@/lib/prefs";
import type { Format } from "./types";

export const DPI_MIN = 72;
export const DPI_MAX = 600;
export const DPI_DEFAULT = 150;

export const prefs = definePrefs(
  "tool:pdf-to-images",
  { format: "png", dpi: DPI_DEFAULT },
  {
    format: oneOf<Format>(["png", "jpeg"]),
    // Not range-clamped on purpose: the field is free-typed, and Rust
    // clamps DPI into [72, 600] in `convert` anyway.
    dpi: finiteNumber({ integer: true }),
  },
);
