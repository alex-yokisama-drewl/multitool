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

// Image Format Converter's broader picker. The mock returns the same set
// as pickImageFiles — the converter doesn't care about ordering, so any
// non-empty list drives the staging flow.
export function pickConvertibleImages(): Promise<string[] | null> {
  return Promise.resolve(MOCK_IMAGE_PATHS);
}

// Audio Format Converter's picker. Mock returns three deliberately-named
// audio paths so the staging UI has rows to render.
const MOCK_AUDIO_PATHS = [
  "/tmp/multitool-e2e/song.wav",
  "/tmp/multitool-e2e/track.mp3",
  "/tmp/multitool-e2e/album.flac",
];

export function pickConvertibleAudio(): Promise<string[] | null> {
  return Promise.resolve(MOCK_AUDIO_PATHS);
}

// Audio Trimmer's single-select picker. The mock returns the first audio
// path so the trimmer's `picked` state has something to load.
export function pickAudioFile(): Promise<string | null> {
  return Promise.resolve(MOCK_AUDIO_PATHS[0] ?? null);
}

// Video Format Converter's picker. Two paths so the staging UI has
// rows to render but the conversion stays brisk in the spec timeline.
const MOCK_VIDEO_PATHS = [
  "/tmp/multitool-e2e/holiday.mov",
  "/tmp/multitool-e2e/screencast.mkv",
];

export function pickVideoFiles(): Promise<string[] | null> {
  return Promise.resolve(MOCK_VIDEO_PATHS);
}

// Audio Extractor's single-select picker. Returns the first mock video
// so the extractor's `picked` view has something to render.
export function pickVideoFile(): Promise<string | null> {
  return Promise.resolve(MOCK_VIDEO_PATHS[0] ?? null);
}

// Image Crop's single-select picker. Returns a canned image path; the crop
// preview's `<img>` resolves to the data URL from `imageAssetUrl` below, so
// its `onLoad` fires and the frame editor renders.
export function pickRasterImage(): Promise<string | null> {
  return Promise.resolve("/tmp/multitool-e2e/photo.png");
}

// A real, decodable 1×1 PNG. The Image Crop tool gates its frame editor on
// the preview image's `onLoad` (it reads naturalWidth/Height), so a stub URL
// that 404s wouldn't fire load. A data URL loads in a plain browser; tools
// that only show thumbnails (the converter) are unaffected by the swap.
const ONE_BY_ONE_PNG =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

export function imageAssetUrl(_path: string): string {
  void _path;
  return ONE_BY_ONE_PNG;
}

// Same shim for audio. Trimmer e2e doesn't drive Web Audio decode — the
// spec mocks the trim invoke directly — so a stub URL is enough.
export function audioAssetUrl(path: string): string {
  return `mock-asset://${path}`;
}

export function allowMediaPreview(_paths: string[]): Promise<void> {
  void _paths;
  // No-op in browser — there's no Tauri asset-protocol scope to widen.
  // The browser will simply 404 on `convertFileSrc(path)`; that's fine,
  // the spec doesn't assert on rendered media bytes (alt="" + no-source
  // fallback). Just keep the wrapper resolving so the picker flow
  // continues into staging.
  return Promise.resolve();
}

export function revealInFolder(_path: string): Promise<void> {
  void _path;
  // No-op in browser; the spec only asserts the button is clickable.
  return Promise.resolve();
}
