import type { Tool } from "@/tools/registry";
import { VideoCompress } from "./VideoCompress";

export const videoCompressTool: Tool = {
  id: "video-compress",
  name: "Compress",
  description:
    "Re-encode a video at a chosen quality to produce a smaller file.",
  category: "video",
  color: "violet",
  route: "/tools/video-compress",
  component: VideoCompress,
};
