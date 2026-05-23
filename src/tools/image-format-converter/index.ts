import type { Tool } from "@/tools/registry";
import { ImageFormatConverter } from "./ImageFormatConverter";

export const imageFormatConverterTool: Tool = {
  id: "image-format-converter",
  name: "Format Converter",
  description: "Convert images between formats (PNG, JPEG, WebP, BMP, TIFF).",
  category: "image",
  color: "sky",
  route: "/tools/image-format-converter",
  component: ImageFormatConverter,
};
