import type { Tool } from "@/tools/registry";
import { AudioFormatConverter } from "./AudioFormatConverter";

export const audioFormatConverterTool: Tool = {
  id: "audio-format-converter",
  name: "Format Converter",
  description:
    "Convert audio files between formats (WAV, FLAC, MP3, OGG Vorbis).",
  category: "audio",
  color: "emerald",
  route: "/tools/audio-format-converter",
  component: AudioFormatConverter,
};
