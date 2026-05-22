import type { Tool } from "@/tools/registry";
import { ImageFormatConverter } from "./ImageFormatConverter";

export const imageFormatConverterTool: Tool = {
  id: "image-format-converter",
  name: "Image Format Converter",
  description: "Convert images between formats (PNG, JPEG, WebP, BMP, TIFF).",
  category: "convert",
  route: "/tools/image-format-converter",
  component: ImageFormatConverter,
};
