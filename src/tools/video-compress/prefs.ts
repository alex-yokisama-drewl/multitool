// Remembered options for this tool. The quality slider is a workflow
// preference; preview volume is shared app-wide (see
// `@/lib/playbackPrefs`) because the Video Trimmer shows the same preview.

import { definePrefs, numberIn } from "@/lib/prefs";

export const DEFAULT_QUALITY = 0.5;

export const prefs = definePrefs(
  "tool:video-compress",
  { quality: DEFAULT_QUALITY },
  // Slider-backed, so the control can't produce an out-of-range value.
  { quality: numberIn(0, 1) },
);
