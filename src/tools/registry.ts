import type { ComponentType } from "react";

export type ToolCategory = "convert" | "media" | "text" | "utility";

export interface Tool {
  id: string;
  name: string;
  description: string;
  category: ToolCategory;
  route: string;
  component: ComponentType;
}

export const tools: readonly Tool[] = [];
