// App-wide playback preferences for the in-tool media previews.
//
// Shared rather than per-tool because it is the same preference: the Video
// Trimmer and Video Compress both show a `<video>` preview, and a user who
// turns the sound down once means it for both. This is the second tool to
// need the shape, which is the bar ADDING_A_TOOL sets for extracting to a
// shared surface.
//
// Deliberately not a conversion option — it never reaches Rust. It is here
// so a muted preview stays muted instead of blasting on every pick.

import { definePrefs, numberIn } from "@/lib/prefs";

export const playbackPrefs = definePrefs(
  "app:playback",
  { volume: 1 },
  // Slider-backed and already clamped at the call site, so clamping on
  // read costs nothing.
  { volume: numberIn(0, 1) },
);
