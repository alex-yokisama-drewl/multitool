import type { Tool } from "@/tools/registry";
import { VideoTrimmer } from "./VideoTrimmer";

export const videoTrimmerTool: Tool = {
  id: "video-trimmer",
  name: "Trimmer",
  description: "Cut a video to a frame-accurate range.",
  category: "video",
  color: "rose",
  route: "/tools/video-trimmer",
  component: VideoTrimmer,
};
