import type { ComponentType } from "react";
import { audioExtractorTool } from "./audio-extractor";
import { audioFormatConverterTool } from "./audio-format-converter";
import { audioTrimmerTool } from "./audio-trimmer";
import { imageCropTool } from "./image-crop";
import { imageFormatConverterTool } from "./image-format-converter";
import { imagesToPdfTool } from "./images-to-pdf";
import { loremTool } from "./lorem";
import { pdfToImagesTool } from "./pdf-to-images";
import { videoFormatConverterTool } from "./video-format-converter";
import { videoTrimmerTool } from "./video-trimmer";

export type ToolCategory = "pdf" | "image" | "audio" | "video" | "text";

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
  { id: "audio", label: "Audio" },
  { id: "video", label: "Video" },
  { id: "text", label: "Text" },
];

// Tile colors are CSS-variable tokens defined in src/app/globals.css as
// --tile-<name> + --tile-<name>-fg. Adding a new color = add the token pair
// in globals.css and extend this union. Repeats between tools/categories are
// fine.
export type TileColor =
  | "sky"
  | "amber"
  | "rose"
  | "emerald"
  | "violet"
  | "teal";

export interface Tool {
  id: string;
  name: string;
  description: string;
  category: ToolCategory;
  color: TileColor;
  route: string;
  component: ComponentType;
}

export const tools: readonly Tool[] = [
  imageFormatConverterTool,
  imageCropTool,
  imagesToPdfTool,
  pdfToImagesTool,
  audioFormatConverterTool,
  audioTrimmerTool,
  videoFormatConverterTool,
  videoTrimmerTool,
  audioExtractorTool,
  loremTool,
];
