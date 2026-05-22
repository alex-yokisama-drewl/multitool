import type { ComponentType } from "react";
import { imagesToPdfTool } from "./images-to-pdf";
import { pdfToImagesTool } from "./pdf-to-images";

export type ToolCategory = "convert" | "media" | "text" | "utility";

export interface Tool {
  id: string;
  name: string;
  description: string;
  category: ToolCategory;
  route: string;
  component: ComponentType;
}

export const tools: readonly Tool[] = [imagesToPdfTool, pdfToImagesTool];
