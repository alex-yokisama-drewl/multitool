import type { Tool } from "@/tools/registry";
import { VideoFormatConverter } from "./VideoFormatConverter";

export const videoFormatConverterTool: Tool = {
  id: "video-format-converter",
  name: "Format Converter",
  description: "Convert video files between formats (MP4, WebM, Matroska).",
  category: "video",
  color: "teal",
  route: "/tools/video-format-converter",
  component: VideoFormatConverter,
};
