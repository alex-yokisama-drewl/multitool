// Remembered options for this tool. All five are workflow preferences
// ("I rip everything to 320 kbps MP3"), none describes the picked files.
//
// `flac_compression_level` is absent because it has no UI — the component
// passes a fixed 5 to keep the wire shape stable.

import { definePrefs, finiteNumber, oneOf } from "@/lib/prefs";
import type { ChannelMode, TargetFormat, WavBitDepth } from "./types";

export const prefs = definePrefs(
  "tool:audio-format-converter",
  {
    targetFormat: "mp3",
    mp3BitrateKbps: 192,
    vorbisQuality: 5,
    wavBitDepth: "bit16",
    channels: "source",
  },
  {
    targetFormat: oneOf<TargetFormat>(["wav", "flac", "mp3", "ogg"]),
    wavBitDepth: oneOf<WavBitDepth>(["bit16", "bit24", "bit32f"]),
    channels: oneOf<ChannelMode>(["source", "mono", "stereo"]),
    // Both free-typed, and `buildOpts` already clamps them on the way to
    // Rust — clamping on read would fight the user mid-keystroke.
    mp3BitrateKbps: finiteNumber({ integer: true }),
    vorbisQuality: finiteNumber({ integer: true }),
  },
);
