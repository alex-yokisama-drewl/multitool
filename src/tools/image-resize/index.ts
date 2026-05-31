import type { Tool } from "@/tools/registry";
import { ImageResize } from "./ImageResize";

export const imageResizeTool: Tool = {
  id: "image-resize",
  name: "Resize",
  description:
    "Resize an image to a target width and height, preserving its format.",
  category: "image",
  color: "emerald",
  route: "/tools/image-resize",
  component: ImageResize,
};
