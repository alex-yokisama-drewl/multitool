import type { Tool } from "@/tools/registry";
import { AudioExtractor } from "./AudioExtractor";

export const audioExtractorTool: Tool = {
  id: "audio-extractor",
  name: "Audio Extractor",
  description: "Extract every audio track from a video file as MP3.",
  category: "video",
  color: "amber",
  route: "/tools/audio-extractor",
  component: AudioExtractor,
};
