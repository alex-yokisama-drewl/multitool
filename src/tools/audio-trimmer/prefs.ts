// Remembered options for this tool.
//
// The trim window is absent: start and end are properties of the clip in
// front of the user. The fade toggles are not — "I always fade my clips" is
// a habit, so they stick.

import { bool, definePrefs } from "@/lib/prefs";

export const prefs = definePrefs(
  "tool:audio-trimmer",
  { fadeIn: false, fadeOut: false },
  { fadeIn: bool(), fadeOut: bool() },
);
