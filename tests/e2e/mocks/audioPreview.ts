// E2E-mode replacement for `src/lib/audioPreview.ts`.
//
// vite.config.ts aliases the real module to this file when VITE_E2E=true.
// Module shape MUST match the real one — TypeScript catches drift.
//
// The mock skips the asset-protocol fetch + Web Audio decode entirely:
// returns a canned `AudioPreviewSource` with a small synthetic peaks
// array. The trimmer e2e spec doesn't drive Preview playback, so the
// `audioBuffer` / `audioContext` fields can be opaque stubs the
// component never inspects.

export type {
  Peak,
  AudioPreviewSource,
  PreviewPlayer,
} from "@/lib/audioPreview";

import type { AudioPreviewSource, PreviewPlayer } from "@/lib/audioPreview";

const MOCK_DURATION_MS = 5_000;
const MOCK_BIN_COUNT = 50;

export function loadAudioPreview(
  _path: string,
  _binCount?: number,
): Promise<AudioPreviewSource> {
  void _path;
  void _binCount;
  // Sine-shaped peaks so the waveform renders something visible without
  // depending on a real audio decode.
  const peaks = Array.from({ length: MOCK_BIN_COUNT }, (_, i) => {
    const v = Math.sin((i / MOCK_BIN_COUNT) * Math.PI) * 0.5;
    return { min: -v, max: v };
  });
  return Promise.resolve({
    durationMs: MOCK_DURATION_MS,
    peaks,
    audioBuffer: {} as unknown as AudioBuffer,
    audioContext: {} as unknown as AudioContext,
  });
}

export function createPreviewPlayer(
  _audioContext: AudioContext,
  _audioBuffer: AudioBuffer,
  _options: {
    startMs: number;
    endMs: number;
    fadeInMs: number;
    fadeOutMs: number;
  },
  onStop?: () => void,
): PreviewPlayer {
  // Preview isn't exercised by the happy-path e2e; the handle just has
  // to satisfy the type and let `stop()` be called idempotently.
  void onStop;
  return {
    stop: () => undefined,
  };
}
