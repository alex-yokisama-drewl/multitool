// E2E-mode replacement for `src/lib/system.ts`.
//
// vite.config.ts aliases the real wrapper to this file when VITE_E2E=true.
// Plugin-dialog and plugin-opener don't work in a regular browser, so the
// happy-path spec runs against these stubs.

const MOCK_PDF_PATH = "/tmp/multitool-e2e/sample.pdf";

export function pickPdfFile(): Promise<string | null> {
  return Promise.resolve(MOCK_PDF_PATH);
}

export function revealInFolder(_path: string): Promise<void> {
  void _path;
  // No-op in browser; the spec only asserts the button is clickable.
  return Promise.resolve();
}
