//! Audio Trimmer — pure transform.
//!
//! Owns the IPC wire types ([`Opts`]) and the pure
//! [`trim_and_fade`] function. The orchestrator in `super::job` calls
//! [`trim_and_fade`] between decode + encode.

use serde::{Deserialize, Serialize};

use crate::audio_codecs::AudioBuffer;
use crate::error::{AppError, AppResult};

/// User-facing trim options. Mirrors the form fields on the tool view.
///
/// Range bounds are in milliseconds from the start of the source.
/// `end_ms` is clamped to the source duration in the orchestrator;
/// `start_ms >= end_ms` is rejected pre-encode with a `ProcessingFailed`.
/// Fades are in milliseconds too, clamped to half the trim window when
/// `fade_in_ms + fade_out_ms > (end_ms − start_ms)` — a warning rides
/// along on the success event.
///
/// The UI exposes fades as checkboxes that toggle a fixed `1000 ms`
/// default value; the Rust API keeps the millisecond field so unit
/// tests can hit edge cases (`0`, equal-to-window, overlap) without
/// going through the UI clamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Opts {
    pub start_ms: u64,
    pub end_ms: u64,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
}

/// Trim `buf` to `[start_ms, end_ms]` and apply optional linear
/// fade-in / fade-out at the edges.
///
/// Range rules:
/// - `end_ms` is clamped silently to the source's actual duration. The
///   UI's source duration is advisory; a renamed-extension file can
///   produce a shorter buffer than the picker reported, and we'd rather
///   trim the available range than reject.
/// - `start_ms >= end_ms` (after clamping) is rejected with
///   `ProcessingFailed`. Empty trim makes no sense and the encoder pass
///   would just produce a zero-sample file anyway.
///
/// Fade rules:
/// - Linear ramp 0→1 across `fade_in_ms` worth of frames at the start
///   of the trimmed region; linear ramp 1→0 across `fade_out_ms` at the
///   end. Each frame's gain multiplies every channel equally
///   (per-frame, not per-sample), so the channel image isn't distorted.
/// - When `fade_in_ms + fade_out_ms > trim_len_ms`, each fade is
///   clamped to half the trim window and a warning is emitted. The
///   policy is symmetric on purpose — without knowing user intent,
///   chopping both fades in half preserves the relative shape rather
///   than starving one side.
/// - Zero-ms fades pass through unchanged (no warning, no work).
///
/// The returned buffer keeps the source's channel count and sample rate
/// (no resampling). Warnings ride alongside the buffer so the
/// orchestrator can surface them on the success event without
/// re-deriving them in the caller.
pub fn trim_and_fade(
    buf: &AudioBuffer,
    start_ms: u64,
    end_ms: u64,
    fade_in_ms: u32,
    fade_out_ms: u32,
) -> AppResult<(AudioBuffer, Vec<String>)> {
    if buf.channels == 0 || buf.sample_rate == 0 {
        return Err(AppError::ProcessingFailed {
            detail: format!(
                "audio trim: invalid buffer (channels={}, sample_rate={})",
                buf.channels, buf.sample_rate
            ),
        });
    }

    let channels = usize::from(buf.channels);
    let total_frames = buf.samples.len() / channels;
    // ms = frames × 1000 / sample_rate. Compute in u128 to dodge overflow
    // on hour-long files at 192 kHz.
    let source_duration_ms = ((total_frames as u128) * 1000)
        .checked_div(u128::from(buf.sample_rate))
        .unwrap_or(0);
    let source_duration_ms = u64::try_from(source_duration_ms).unwrap_or(u64::MAX);

    let end_ms = end_ms.min(source_duration_ms);

    if start_ms >= end_ms {
        return Err(AppError::ProcessingFailed {
            detail: format!(
                "audio trim: invalid range (start_ms={start_ms} >= end_ms={end_ms}, source duration {source_duration_ms} ms)"
            ),
        });
    }

    let sample_rate = buf.sample_rate;
    let start_frame = ms_to_frames(start_ms, sample_rate);
    let end_frame = ms_to_frames(end_ms, sample_rate).min(total_frames);
    if end_frame <= start_frame {
        return Err(AppError::ProcessingFailed {
            detail: format!(
                "audio trim: zero-frame range after rounding (start_frame={start_frame}, end_frame={end_frame})"
            ),
        });
    }
    let trim_frame_count = end_frame - start_frame;
    let trim_len_ms = end_ms - start_ms;

    // Overlap clamp. The sum-overflow check is theoretical (u32 + u32
    // fits in u64); doing it anyway keeps the comparison total.
    let mut warnings = Vec::new();
    let fade_sum = u64::from(fade_in_ms) + u64::from(fade_out_ms);
    let (eff_fade_in_ms, eff_fade_out_ms): (u32, u32) = if fade_sum > trim_len_ms {
        let half = u32::try_from(trim_len_ms / 2).unwrap_or(u32::MAX);
        warnings.push(format!(
            "fade-in/out exceeded trim window of {trim_len_ms} ms; clamped each to {half} ms"
        ));
        (half, half)
    } else {
        (fade_in_ms, fade_out_ms)
    };

    let start_sample = start_frame * channels;
    let end_sample = end_frame * channels;
    let mut samples: Vec<f32> = buf.samples[start_sample..end_sample].to_vec();

    let fade_in_frames = ms_to_frames(u64::from(eff_fade_in_ms), sample_rate).min(trim_frame_count);
    if fade_in_frames > 0 {
        // gain(i) = i / fade_in_frames, ranging 0.0 at i=0 to
        // (fade_in_frames-1)/fade_in_frames at the last fade-in frame.
        // Frame `fade_in_frames` onward is at full amplitude (gain == 1).
        // Choosing this formula (rather than `i / (N-1)`) keeps `gain(0)
        // == 0` literally so the "starts at amplitude 0" invariant tests
        // cleanly.
        let denom = fade_in_frames as f32;
        for frame_idx in 0..fade_in_frames {
            let gain = (frame_idx as f32) / denom;
            let base = frame_idx * channels;
            for c in 0..channels {
                samples[base + c] *= gain;
            }
        }
    }

    let fade_out_frames =
        ms_to_frames(u64::from(eff_fade_out_ms), sample_rate).min(trim_frame_count);
    if fade_out_frames > 0 {
        // Symmetric mirror of fade-in: `k` counts inward from the END.
        // gain(0) corresponds to the LAST frame and is 0.0; gain at the
        // first fade-out frame is (fade_out_frames-1)/fade_out_frames.
        // The very last sample is guaranteed zero — required for the
        // "fade-out ends at 0" acceptance test.
        let denom = fade_out_frames as f32;
        for k in 0..fade_out_frames {
            let frame_idx = trim_frame_count - 1 - k;
            let gain = (k as f32) / denom;
            let base = frame_idx * channels;
            for c in 0..channels {
                samples[base + c] *= gain;
            }
        }
    }

    Ok((
        AudioBuffer {
            samples,
            channels: buf.channels,
            sample_rate: buf.sample_rate,
        },
        warnings,
    ))
}

/// `ms * sample_rate / 1000`, rounded down. u128 intermediate to dodge
/// overflow at multi-hour durations + sample rates up to 192 kHz.
fn ms_to_frames(ms: u64, sample_rate: u32) -> usize {
    let frames = (u128::from(ms) * u128::from(sample_rate)) / 1000;
    usize::try_from(frames).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(samples: Vec<f32>, channels: u16, sample_rate: u32) -> AudioBuffer {
        AudioBuffer {
            samples,
            channels,
            sample_rate,
        }
    }

    /// Construct a 1-second mono buffer at 1000 Hz where each frame's
    /// sample value is `1.0`. Easy to reason about: every sample is the
    /// same so any change must come from trim/fade math.
    fn flat_mono_1s_1khz() -> AudioBuffer {
        buffer(vec![1.0; 1000], 1, 1000)
    }

    #[test]
    fn trim_extracts_the_requested_subrange_with_no_fades() {
        // Source: 1.0 second of 1.0-valued mono samples at 1 kHz.
        let src = flat_mono_1s_1khz();
        let (out, warnings) = trim_and_fade(&src, 200, 700, 0, 0).expect("trim ok");
        // 500 ms × 1000 Hz = 500 frames; mono → 500 samples.
        assert_eq!(out.samples.len(), 500);
        assert_eq!(out.channels, 1);
        assert_eq!(out.sample_rate, 1000);
        // No fades → all samples should still be 1.0.
        assert!(out.samples.iter().all(|&s| (s - 1.0).abs() < 1e-6));
        assert!(warnings.is_empty());
    }

    #[test]
    fn fade_in_starts_at_amplitude_zero_and_ramps_to_unity() {
        let src = flat_mono_1s_1khz();
        // Trim 0..500 ms, fade-in 100 ms (= 100 frames).
        let (out, _) = trim_and_fade(&src, 0, 500, 100, 0).expect("trim ok");
        assert_eq!(out.samples.len(), 500);
        // First sample == 0.0 (the acceptance criterion).
        assert!(out.samples[0].abs() < 1e-6, "got {}", out.samples[0]);
        // Mid-fade: i=50 → gain 50/100 = 0.5.
        assert!(
            (out.samples[50] - 0.5).abs() < 1e-6,
            "got {}",
            out.samples[50]
        );
        // Just before fade ends: i=99 → gain 99/100 = 0.99.
        assert!(
            (out.samples[99] - 0.99).abs() < 1e-4,
            "got {}",
            out.samples[99]
        );
        // Post-fade: gain 1.0 unchanged.
        assert!((out.samples[100] - 1.0).abs() < 1e-6);
        assert!((out.samples[499] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fade_out_ends_at_amplitude_zero_and_ramps_down_from_unity() {
        let src = flat_mono_1s_1khz();
        // Trim 0..500 ms, fade-out 100 ms (= 100 frames at the tail).
        let (out, _) = trim_and_fade(&src, 0, 500, 0, 100).expect("trim ok");
        assert_eq!(out.samples.len(), 500);
        // Pre-fade-out region: untouched.
        assert!((out.samples[0] - 1.0).abs() < 1e-6);
        assert!((out.samples[399] - 1.0).abs() < 1e-6);
        // Start of fade-out: 100 frames before the end. k = 99 → gain 99/100.
        assert!(
            (out.samples[400] - 0.99).abs() < 1e-4,
            "got {}",
            out.samples[400]
        );
        // Mid-fade: 50 frames before the end. k = 49 → gain 49/100 = 0.49.
        assert!(
            (out.samples[450] - 0.49).abs() < 1e-4,
            "got {}",
            out.samples[450]
        );
        // Last sample: k = 0 → gain 0 → silence (the acceptance criterion).
        assert!(out.samples[499].abs() < 1e-6, "got {}", out.samples[499]);
    }

    #[test]
    fn invalid_range_start_ge_end_returns_processing_failed() {
        let src = flat_mono_1s_1khz();
        let result = trim_and_fade(&src, 500, 500, 0, 0);
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(detail.contains("invalid range"), "got: {detail}");
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }

        let result = trim_and_fade(&src, 800, 500, 0, 0);
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }

    #[test]
    fn end_ms_beyond_source_duration_is_clamped_silently() {
        let src = flat_mono_1s_1khz(); // 1000 ms
                                       // Ask for 0..5000 ms — should clamp to 0..1000 silently.
        let (out, warnings) = trim_and_fade(&src, 0, 5_000, 0, 0).expect("trim ok");
        assert_eq!(out.samples.len(), 1000);
        // No warnings — clamping `end_ms` is silent per the spec.
        assert!(warnings.is_empty());
    }

    #[test]
    fn fade_overlap_clamps_each_to_half_and_emits_a_warning() {
        let src = flat_mono_1s_1khz();
        // Trim is 400 ms; fade_in + fade_out = 500 > 400 → clamp each to 200.
        let (out, warnings) = trim_and_fade(&src, 0, 400, 300, 200).expect("trim ok despite clamp");
        assert_eq!(out.samples.len(), 400);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("clamped each to 200 ms"),
            "got: {}",
            warnings[0]
        );
        // Fade-in clamped to 200 ms = 200 frames; sample[0] = 0,
        // sample[100] = 100/200 = 0.5, sample[199] = 199/200 = 0.995,
        // sample[200] would be 1.0 ... except fade-out clamped to 200
        // starts at frame 200 (last 200 of 400) and immediately ramps
        // back down: sample[200] = gain (k=199) = 199/200 = 0.995.
        assert!(out.samples[0].abs() < 1e-6);
        assert!((out.samples[100] - 0.5).abs() < 1e-6);
        assert!((out.samples[199] - 0.995).abs() < 1e-4);
        // Boundary: fade-in's gain @ 200 would be 1.0; fade-out's gain
        // @ 200 (k=199) is 199/200 = 0.995. Fade-out is applied AFTER
        // fade-in, multiplying, so the actual value is 1.0 × 0.995.
        assert!((out.samples[200] - 0.995).abs() < 1e-4);
        // Tail of fade-out → silence.
        assert!(out.samples[399].abs() < 1e-6);
    }

    #[test]
    fn fade_overlap_with_only_one_fade_set_still_clamps_when_it_exceeds_window() {
        // Per the brief: clamp triggers on SUM > window. fade_in_ms=600
        // with fade_out_ms=0 and a 400 ms window → sum=600 > 400 →
        // clamp each to 200.
        let src = flat_mono_1s_1khz();
        let (_, warnings) = trim_and_fade(&src, 0, 400, 600, 0).expect("trim ok");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("clamped each to 200 ms"));
    }

    #[test]
    fn multi_channel_fade_scales_every_channel_in_a_frame_equally() {
        // 2-channel stereo where L is 1.0 and R is 0.5 across every frame.
        // 1 second @ 1 kHz = 1000 frames × 2 channels = 2000 samples.
        let mut samples = Vec::with_capacity(2000);
        for _ in 0..1000 {
            samples.push(1.0); // L
            samples.push(0.5); // R
        }
        let src = buffer(samples, 2, 1000);

        // Trim 0..500 ms with fade-in 100 ms.
        let (out, _) = trim_and_fade(&src, 0, 500, 100, 0).expect("trim ok");
        assert_eq!(out.channels, 2);
        // 500 frames × 2 channels = 1000 samples.
        assert_eq!(out.samples.len(), 1000);

        // Frame 0: L = 0 × 1.0 = 0, R = 0 × 0.5 = 0.
        assert!(out.samples[0].abs() < 1e-6);
        assert!(out.samples[1].abs() < 1e-6);
        // Frame 50 (mid-fade, gain 0.5): L = 0.5 × 1.0 = 0.5, R = 0.5 × 0.5 = 0.25.
        assert!(
            (out.samples[100] - 0.5).abs() < 1e-6,
            "got L={}",
            out.samples[100]
        );
        assert!(
            (out.samples[101] - 0.25).abs() < 1e-6,
            "got R={}",
            out.samples[101]
        );
        // Frame 100+ (post-fade): L = 1.0, R = 0.5 preserved.
        assert!((out.samples[200] - 1.0).abs() < 1e-6);
        assert!((out.samples[201] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn zero_fades_dont_warn_and_dont_touch_samples() {
        let src = flat_mono_1s_1khz();
        let (out, warnings) = trim_and_fade(&src, 100, 900, 0, 0).expect("trim ok");
        assert_eq!(out.samples.len(), 800);
        assert!(out.samples.iter().all(|&s| (s - 1.0).abs() < 1e-6));
        assert!(warnings.is_empty());
    }

    #[test]
    fn fade_in_equal_to_trim_window_uses_full_window_without_overlap_warning() {
        // fade_in 500 + fade_out 0 = 500, trim 0..500 = 500 ms. Sum == window
        // → not strictly greater → no clamp, no warning.
        let src = flat_mono_1s_1khz();
        let (out, warnings) = trim_and_fade(&src, 0, 500, 500, 0).expect("trim ok");
        assert!(warnings.is_empty());
        // First sample 0, last sample at gain 499/500 ≈ 0.998.
        assert!(out.samples[0].abs() < 1e-6);
        assert!((out.samples[499] - 0.998).abs() < 1e-3);
    }

    #[test]
    fn zero_channel_buffer_is_rejected() {
        let src = buffer(vec![], 0, 44100);
        let result = trim_and_fade(&src, 0, 100, 0, 0);
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }

    #[test]
    fn zero_sample_rate_buffer_is_rejected() {
        let src = buffer(vec![0.0; 100], 1, 0);
        let result = trim_and_fade(&src, 0, 100, 0, 0);
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }

    #[test]
    fn channels_and_sample_rate_pass_through_unchanged() {
        let src = buffer(vec![0.5; 88_200], 2, 44_100); // 1 second of stereo at 44.1 kHz
        let (out, _) = trim_and_fade(&src, 100, 900, 0, 0).expect("trim ok");
        assert_eq!(out.channels, 2);
        assert_eq!(out.sample_rate, 44_100);
        // 800 ms × 44_100 Hz = 35_280 frames; × 2 channels = 70_560 samples.
        assert_eq!(out.samples.len(), 70_560);
    }
}
