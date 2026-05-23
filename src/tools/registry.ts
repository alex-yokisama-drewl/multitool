import type { ComponentType } from "react";
import { imageFormatConverterTool } from "./image-format-converter";
import { imagesToPdfTool } from "./images-to-pdf";
import { pdfToImagesTool } from "./pdf-to-images";

export type ToolCategory = "pdf" | "image";

export interface ToolCategoryMeta {
  id: ToolCategory;
  label: string;
}

// Display order on the dashboard. Adding a brand-new category means extending
// both ToolCategory and this list; adding a tool that fits an existing category
// requires no edits here.
export const toolCategories: readonly ToolCategoryMeta[] = [
  { id: "pdf", label: "PDF" },
  { id: "image", label: "Image" },
];

export interface Tool {
  id: string;
  name: string;
  description: string;
  category: ToolCategory;
  route: string;
  component: ComponentType;
}

export const tools: readonly Tool[] = [
  imageFormatConverterTool,
  imagesToPdfTool,
  pdfToImagesTool,
];
