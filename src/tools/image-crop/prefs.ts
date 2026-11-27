// Remembered options for this tool.
//
// The crop rect is deliberately absent: it is a property of the image in
// front of the user, and carrying it to the next image would be nonsense.
// The lock *toggle* is a different thing — it is how the user likes to
// drag — so it sticks. Only the boolean persists, never the captured
// ratio: that is re-derived from whichever image just loaded.

import { bool, definePrefs } from "@/lib/prefs";

export const prefs = definePrefs(
  "tool:image-crop",
  { lockProportions: false },
  { lockProportions: bool() },
);
