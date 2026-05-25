// Browser-side audio preview helpers for the Audio Trimmer.
//
// The trimmer needs two things the Rust side can't cheaply give us:
//   1. A peaks array for the waveform canvas (visual marker placement).
//   2. Approximate-fade preview playback without re-encoding per click.
//
// Both are served from a single `AudioBuffer` produced by Web Audio's
// `decodeAudioData`. The asset URL routes through Tauri's asset
// protocol; the trimmer must call `allowMediaPreview([path])` first to
// widen the per-pick scope (see DECISIONS.md → "Asset protocol scope").
//
// Tests mock this module wholesale via vi.mock("@/lib/audioPreview") —
// jsdom lacks AudioContext and the actual decode would error.

import { audioAssetUrl } from "./system";

/// A single waveform bin's min/max f32 sample value across the bin's
/// frame range. The canvas draws a vertical bar from `min` to `max`
/// in [-1, 1] space.
export interface Peak {
  min: number;
  max: number;
}

/// Data returned by [`loadAudioPreview`] — everything the UI needs to
/// render the waveform and drive Web Audio preview playback.
export interface AudioPreviewSource {
  /// Total duration in milliseconds. Computed from `audioBuffer.length
  /// / audioBuffer.sampleRate × 1000` (and rounded), so it agrees with
  /// the Rust side's `frames * 1000 / sample_rate` derivation.
  durationMs: number;
  /// `binCount`-long peaks array, one entry per bin in source order.
  /// Mixed to mono before peak extraction so multi-channel sources
  /// don't widen the rendered bars artificially.
  peaks: Peak[];
  /// The decoded AudioBuffer — passed straight to
  /// [`createPreviewPlayer`] for playback.
  audioBuffer: AudioBuffer;
  /// The AudioContext that produced `audioBuffer`. Re-used for
  /// playback so we don't need a second context per page.
  audioContext: AudioContext;
}

/// Default bin count for the waveform canvas. 1000 bins comfortably
/// covers a 4K canvas at 1 bar per ~4 pixels; sub-pixel resolution is
/// pointless for a "did I pick the right region" affordance.
const DEFAULT_BIN_COUNT = 1000;

/// Fetch the audio bytes via Tauri's asset protocol and decode through
/// Web Audio. Returns peaks + the AudioBuffer for the trimmer's UI to
/// drive both the waveform render and the preview playback.
///
/// Caller MUST have called `allowMediaPreview([path])` first — without
/// the per-pick scope grant `fetch(audioAssetUrl(path))` resolves to a
/// 403/NotAllowed and the decode rejects.
export async function loadAudioPreview(
  path: string,
  binCount: number = DEFAULT_BIN_COUNT,
): Promise<AudioPreviewSource> {
  const url = audioAssetUrl(path);
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(
      `audio preview: fetch failed (${response.status.toString()})`,
    );
  }
  const arrayBuffer = await response.arrayBuffer();
  // `AudioContext` is constructed eagerly; some browsers start it
  // suspended pending a user gesture. The picker click that lands us
  // here qualifies, but `resume()` is safe to call regardless.
  const audioContext = new AudioContext();
  if (audioContext.state === "suspended") {
    await audioContext.resume();
  }
  const audioBuffer = await audioContext.decodeAudioData(arrayBuffer);

  const durationMs = Math.round(
    (audioBuffer.length / audioBuffer.sampleRate) * 1000,
  );
  const peaks = computePeaks(audioBuffer, binCount);

  return { durationMs, peaks, audioBuffer, audioContext };
}

/// Mix all channels to mono and emit min/max per bin. f32 samples in
/// [-1, 1] are returned unmodified — the canvas draws bars in that
/// range directly.
function computePeaks(audioBuffer: AudioBuffer, binCount: number): Peak[] {
  const frames = audioBuffer.length;
  const nChannels = audioBuffer.numberOfChannels;
  // Pull each channel's Float32Array up front so the inner per-frame
  // loop doesn't repeat the `getChannelData` index lookup.
  const channels: Float32Array[] = [];
  for (let c = 0; c < nChannels; c += 1) {
    channels.push(audioBuffer.getChannelData(c));
  }
  const bins: Peak[] = [];
  // Use a float step + Math.floor on the bin boundaries so trailing
  // frames don't get truncated when `frames % binCount != 0`.
  const step = frames / binCount;
  for (let bin = 0; bin < binCount; bin += 1) {
    const begin = Math.floor(bin * step);
    const endExclusive = Math.min(frames, Math.floor((bin + 1) * step));
    let min = 0;
    let max = 0;
    for (let i = begin; i < endExclusive; i += 1) {
      // Mono mix: equal-weight average across channels for the peak
      // calculation. Matches the Rust converter's `downmix_to_mono`
      // shape so the rendered waveform aligns with what gets trimmed.
      let mixed = 0;
      for (let c = 0; c < nChannels; c += 1) {
        mixed += channels[c]![i]!;
      }
      mixed /= nChannels;
      if (mixed < min) min = mixed;
      if (mixed > max) max = mixed;
    }
    bins.push({ min, max });
  }
  return bins;
}

/// Imperative handle returned by [`createPreviewPlayer`]. The caller
/// retains it to stop playback or query state.
export interface PreviewPlayer {
  /// Stop playback and disconnect the audio graph. Idempotent.
  stop: () => void;
}

/// Schedule looped preview playback of the `[startMs, endMs]` window
/// with linear gain ramps approximating the encoder's fade-in /
/// fade-out math.
///
/// Loop strategy: schedule one play per loop iteration. When the
/// `AudioBufferSourceNode` fires `onended`, the next play kicks off
/// — unless `stop()` was called, in which case the chain unwinds. This
/// is simpler than `source.loop = true` because the latter doesn't
/// re-trigger the gain envelope each iteration.
///
/// Fade overlap is clamped to half the window, mirroring the Rust
/// side's `trim_and_fade` policy. Approximate by design (browser
/// scheduler precision); the user said preview fades don't need to be
/// pixel-perfect.
export function createPreviewPlayer(
  audioContext: AudioContext,
  audioBuffer: AudioBuffer,
  options: {
    startMs: number;
    endMs: number;
    fadeInMs: number;
    fadeOutMs: number;
  },
  onStop?: () => void,
): PreviewPlayer {
  const {
    startMs,
    endMs,
    fadeInMs: rawFadeIn,
    fadeOutMs: rawFadeOut,
  } = options;

  const windowMs = Math.max(0, endMs - startMs);
  // Same symmetric clamp as the Rust orchestrator.
  let fadeInMs = rawFadeIn;
  let fadeOutMs = rawFadeOut;
  if (fadeInMs + fadeOutMs > windowMs) {
    fadeInMs = Math.floor(windowMs / 2);
    fadeOutMs = Math.floor(windowMs / 2);
  }

  let active: AudioBufferSourceNode | null = null;
  let stopped = false;

  const playOnce = () => {
    if (stopped) return;
    const source = audioContext.createBufferSource();
    source.buffer = audioBuffer;
    const gain = audioContext.createGain();
    source.connect(gain).connect(audioContext.destination);

    const now = audioContext.currentTime;
    const startS = startMs / 1000;
    const windowS = windowMs / 1000;
    const fadeInS = fadeInMs / 1000;
    const fadeOutS = fadeOutMs / 1000;

    if (fadeInS > 0) {
      gain.gain.setValueAtTime(0, now);
      gain.gain.linearRampToValueAtTime(1, now + fadeInS);
    } else {
      gain.gain.setValueAtTime(1, now);
    }
    if (fadeOutS > 0) {
      // Hold at 1 until the fade-out begins, then ramp down.
      const fadeOutBegin = now + Math.max(0, windowS - fadeOutS);
      gain.gain.setValueAtTime(1, fadeOutBegin);
      gain.gain.linearRampToValueAtTime(0, now + windowS);
    }

    source.onended = () => {
      if (stopped) return;
      // Disconnect the finished node and chain another play.
      try {
        source.disconnect();
      } catch {
        // Already disconnected.
      }
      playOnce();
    };

    active = source;
    source.start(0, startS, windowS);
  };

  playOnce();

  return {
    stop: () => {
      if (stopped) return;
      stopped = true;
      if (active) {
        try {
          active.stop();
        } catch {
          // Source may already have ended.
        }
        try {
          active.disconnect();
        } catch {
          // Already disconnected.
        }
        active = null;
      }
      onStop?.();
    },
  };
}
