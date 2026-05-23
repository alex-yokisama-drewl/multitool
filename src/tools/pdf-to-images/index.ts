import type { Tool } from "@/tools/registry";
import { PdfToImages } from "./PdfToImages";

export const pdfToImagesTool: Tool = {
  id: "pdf-to-images",
  name: "PDF → Images",
  description: "Render each page of a PDF as PNG or JPEG.",
  category: "pdf",
  route: "/tools/pdf-to-images",
  component: PdfToImages,
};
