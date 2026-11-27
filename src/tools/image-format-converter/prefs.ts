// Remembered options for this tool. Lives in the tool's own folder so the
// registry contract holds — nothing shared changes to give a tool memory.
//
// Every option here is a workflow preference ("I always convert to WebP and
// flatten alpha onto white"), none is derived from the picked files, so all
// of them stick. `svgSizeMode` and `svgLongestEdgePx` are tracked separately
// for the same reason the component does: toggling the radio must not lose
// the px value the user typed.

import { definePrefs, finiteNumber, oneOf } from "@/lib/prefs";
import type { AlphaHandling, TargetFormat } from "./types";

export type SvgSizeMode = "natural" | "longest-edge-px";

export const prefs = definePrefs(
  "tool:image-format-converter",
  {
    targetFormat: "png",
    jpegQuality: 85,
    alphaHandling: "flatten-white",
    svgSizeMode: "longest-edge-px",
    svgLongestEdgePx: 1024,
  },
  {
    targetFormat: oneOf<TargetFormat>(["png", "jpeg", "webp", "bmp", "tiff"]),
    alphaHandling: oneOf<AlphaHandling>([
      "preserve",
      "flatten-white",
      "flatten-black",
    ]),
    svgSizeMode: oneOf<SvgSizeMode>(["natural", "longest-edge-px"]),
    // Both fields are free-typed, and `buildOpts` already clamps them on the
    // way to Rust — clamping on read would fight the user mid-keystroke.
    jpegQuality: finiteNumber({ integer: true }),
    svgLongestEdgePx: finiteNumber({ integer: true }),
  },
);
