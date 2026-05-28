import type { Tool } from "@/tools/registry";
import { Lorem } from "./Lorem";

export const loremTool: Tool = {
  id: "lorem",
  name: "Lorem Ipsum",
  description: "Generate placeholder text. Copy or regenerate.",
  category: "text",
  color: "teal",
  route: "/tools/lorem",
  component: Lorem,
};
