// Remembered options for this tool. One slider, and it is a pure workflow
// preference ("I compress hard for the web") rather than anything about the
// files in front of the user — so it sticks.

import { definePrefs, numberIn } from "@/lib/prefs";

export const DEFAULT_QUALITY = 0.8;

export const prefs = definePrefs(
  "tool:image-compress",
  { quality: DEFAULT_QUALITY },
  // Slider-backed, so the control can't produce an out-of-range value and
  // clamping on read costs nothing.
  { quality: numberIn(0, 1) },
);
