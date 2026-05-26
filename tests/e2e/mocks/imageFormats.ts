// E2E-mode replacement for `src/lib/imageFormats.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// The real wrapper calls the `supported_raster_formats` Tauri command, which
// isn't available in a plain browser — so the happy-path specs get this
// static descriptor list instead. Mirrors the backend's five encodable
// formats so the converter's target radios + the crop picker filter render.

export interface RasterFormatDescriptor {
  id: string;
  name: string;
  extensions: string[];
  supports_alpha: boolean;
}

const FORMATS: RasterFormatDescriptor[] = [
  { id: "png", name: "PNG", extensions: ["png"], supports_alpha: true },
  {
    id: "jpeg",
    name: "JPEG",
    extensions: ["jpg", "jpeg"],
    supports_alpha: false,
  },
  { id: "webp", name: "WebP", extensions: ["webp"], supports_alpha: true },
  { id: "bmp", name: "BMP", extensions: ["bmp"], supports_alpha: false },
  {
    id: "tiff",
    name: "TIFF",
    extensions: ["tif", "tiff"],
    supports_alpha: true,
  },
];

export function getRasterFormats(): Promise<RasterFormatDescriptor[]> {
  return Promise.resolve(FORMATS);
}

export function rasterImageExtensions(): Promise<string[]> {
  return Promise.resolve(FORMATS.flatMap((f) => f.extensions));
}

export function __resetRasterFormatsCache(): void {
  // No-op: the mock has no cache to clear.
}
