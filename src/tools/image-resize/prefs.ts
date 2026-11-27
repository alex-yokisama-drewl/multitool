// Remembered options for this tool.
//
// `width` / `height` are absent on purpose: they auto-populate from the
// source image's natural dimensions, so a remembered value would fight that
// on every pick. `background` is absent for the same reason — it is
// defaulted per picked file from whether the source format carries alpha
// (see the effect in ImageResize.tsx), so persisting it would be clobbered
// immediately.
//
// What's left is the two genuine workflow preferences. As in Image Crop,
// only the lock's boolean persists; the captured ratio is re-derived from
// whichever image just loaded.

import { bool, definePrefs, oneOf } from "@/lib/prefs";
import type { FitMode } from "./types";

export const prefs = definePrefs(
  "tool:image-resize",
  { fit: "contain", lockAspect: true },
  {
    fit: oneOf<FitMode>(["stretch", "contain", "cover"]),
    lockAspect: bool(),
  },
);
