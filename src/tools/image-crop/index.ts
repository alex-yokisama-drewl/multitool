import type { Tool } from "@/tools/registry";
import { ImageCrop } from "./ImageCrop";

export const imageCropTool: Tool = {
  id: "image-crop",
  name: "Crop",
  description: "Crop an image to a rectangular region, preserving its format.",
  category: "image",
  color: "violet",
  route: "/tools/image-crop",
  component: ImageCrop,
};
