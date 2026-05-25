import type { Tool } from "@/tools/registry";
import { AudioTrimmer } from "./AudioTrimmer";

export const audioTrimmerTool: Tool = {
  id: "audio-trimmer",
  name: "Trimmer",
  description:
    "Cut an audio clip to a range with optional linear fade-in / fade-out.",
  category: "audio",
  color: "violet",
  route: "/tools/audio-trimmer",
  component: AudioTrimmer,
};
