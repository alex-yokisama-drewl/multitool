import type { Tool } from "@/tools/registry";
import { ImagesToPdf } from "./ImagesToPdf";

export const imagesToPdfTool: Tool = {
  id: "images-to-pdf",
  name: "Images → PDF",
  description: "Assemble images into a single PDF, one image per page.",
  category: "pdf",
  color: "amber",
  route: "/tools/images-to-pdf",
  component: ImagesToPdf,
};
