import type { Tool } from "@/tools/registry";
import { ImageCompress } from "./ImageCompress";

export const imageCompressTool: Tool = {
  id: "image-compress",
  name: "Compress",
  description: "Re-encode images at a chosen quality to produce smaller files.",
  category: "image",
  color: "amber",
  route: "/tools/image-compress",
  component: ImageCompress,
};
