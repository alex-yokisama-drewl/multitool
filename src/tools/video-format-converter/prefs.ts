// Remembered options for this tool. One option, and it is a pure workflow
// preference ("everything goes to mp4").

import { definePrefs, oneOf } from "@/lib/prefs";
import type { TargetFormat } from "./types";

export const prefs = definePrefs(
  "tool:video-format-converter",
  { targetFormat: "mp4" },
  { targetFormat: oneOf<TargetFormat>(["mp4", "webm", "mkv"]) },
);
