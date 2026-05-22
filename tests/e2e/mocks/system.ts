// E2E-mode replacement for `src/lib/system.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Plugin-dialog and plugin-opener don't work in a regular browser, so the
// happy-path spec runs against these stubs.

const MOCK_PDF_PATH = "/tmp/multitool-e2e/sample.pdf";

// Picked deliberately in non-alphabetical order so the sort-by-filename
// step in the staging view has something to do (and tests can observe).
const MOCK_IMAGE_PATHS = [
  "/tmp/multitool-e2e/charlie.png",
  "/tmp/multitool-e2e/alpha.jpg",
  "/tmp/multitool-e2e/bravo.webp",
];

export function pickPdfFile(): Promise<string | null> {
  return Promise.resolve(MOCK_PDF_PATH);
}

export function pickImageFiles(): Promise<string[] | null> {
  return Promise.resolve(MOCK_IMAGE_PATHS);
}

export function imageAssetUrl(path: string): string {
  // No Tauri asset protocol in a plain browser — return a harmless URL
  // so `<img>` rendering doesn't blow up. The spec doesn't assert on
  // pixels; alt="" + a broken URL is fine.
  return `mock-asset://${path}`;
}

export function allowImagePreview(_paths: string[]): Promise<void> {
  void _paths;
  // No-op in browser — there's no Tauri asset-protocol scope to widen.
  // The browser will simply 404 on `convertFileSrc(path)`; that's fine,
  // the spec doesn't assert on rendered image bytes (alt="" + no-source
  // fallback). Just keep the wrapper resolving so the picker flow
  // continues into staging.
  return Promise.resolve();
}

export function revealInFolder(_path: string): Promise<void> {
  void _path;
  // No-op in browser; the spec only asserts the button is clickable.
  return Promise.resolve();
}
