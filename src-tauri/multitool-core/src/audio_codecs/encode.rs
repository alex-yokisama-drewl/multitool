//! Encode an [`AudioBuffer`] as WAV / FLAC / MP3 / OGG Vorbis.
//!
//! One pub fn per format; helpers (error mapping, ladder lookup,
//! quality-scale remap) stay private. Plus [`validate_mp3_sample_rate`]
//! for the LAME-set gate that callers apply *before* hitting [`encode_mp3`].
//!
//! No allocator gymnastics — encoders accept the full PCM buffer in one
//! pass. Cancellation between decode and encode is the caller's job; this
//! module exposes pure functions only.

use std::io::Cursor;
use std::mem::MaybeUninit;
use std::num::{NonZeroU32, NonZeroU8};

use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use hound::{SampleFormat as HoundSampleFormat, WavSpec, WavWriter};
use mp3lame_encoder::{Bitrate, Builder as LameBuilder, FlushNoGap, InterleavedPcm, MonoPcm};
use serde::{Deserialize, Serialize};
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

use crate::audio_codecs::AudioBuffer;
use crate::error::{AppError, AppResult};

/// WAV-only: sample bit depth. PCM 16/24 or 32-bit float.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WavBitDepth {
    Bit16,
    Bit24,
    Bit32f,
}

/// Encode an `AudioBuffer` as WAV at the requested bit depth via `hound`.
///
/// Sample-format conversion:
/// - `Bit16`: f32 [−1.0, 1.0] → i16 [`i16::MIN`, `i16::MAX`]. Out-of-range
///   floats are clamped before scaling (sources from lossy decoders can
///   excursion past ±1.0 on transients).
/// - `Bit24`: f32 → 24-bit signed PCM. hound takes i32 with
///   `bits_per_sample = 24`; we scale to [`I24_MIN`, `I24_MAX`].
/// - `Bit32f`: f32 written through directly, `sample_format = Float`.
///
/// Mono / stereo / N-channel are all written as-is — the WAV format
/// carries any channel count. Callers pass interleaved samples; hound
/// expects interleaved input for `write_sample` calls.
pub fn encode_wav(buf: &AudioBuffer, bit_depth: WavBitDepth) -> AppResult<Vec<u8>> {
    let (bits_per_sample, sample_format) = match bit_depth {
        WavBitDepth::Bit16 => (16, HoundSampleFormat::Int),
        WavBitDepth::Bit24 => (24, HoundSampleFormat::Int),
        WavBitDepth::Bit32f => (32, HoundSampleFormat::Float),
    };
    let spec = WavSpec {
        channels: buf.channels,
        sample_rate: buf.sample_rate,
        bits_per_sample,
        sample_format,
    };

    // `Cursor<Vec<u8>>` provides `Write + Seek`, which is what `WavWriter`
    // needs to back-patch the RIFF chunk sizes on `finalize`.
    let mut cursor = Cursor::new(Vec::<u8>::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec).map_err(hound_to_app_err)?;
        match bit_depth {
            WavBitDepth::Bit16 => {
                for &sample in &buf.samples {
                    let s = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                    writer.write_sample(s).map_err(hound_to_app_err)?;
                }
            }
            WavBitDepth::Bit24 => {
                for &sample in &buf.samples {
                    let s = (sample.clamp(-1.0, 1.0) * (I24_MAX as f32)) as i32;
                    writer.write_sample(s).map_err(hound_to_app_err)?;
                }
            }
            WavBitDepth::Bit32f => {
                for &sample in &buf.samples {
                    writer.write_sample(sample).map_err(hound_to_app_err)?;
                }
            }
        }
        writer.finalize().map_err(hound_to_app_err)?;
    }
    Ok(cursor.into_inner())
}

/// Signed 24-bit integer range. hound accepts i32 with bits_per_sample=24,
/// but the actual valid range is [-2^23, 2^23 − 1]. Out-of-range values
/// produce a hound `TooWide` error.
const I24_MAX: i32 = (1 << 23) - 1;

fn hound_to_app_err(err: hound::Error) -> AppError {
    AppError::ProcessingFailed {
        detail: format!("wav encode: {err}"),
    }
}

/// Encode an `AudioBuffer` as FLAC via `flacenc`.
///
/// `_compression_level` is accepted for forward-compat but currently
/// unused — flacenc's Encoder default doesn't honour a single level
/// knob; it has fine-grained `subframe_coding` / `stereo_coding` config
/// blocks instead. v1 ships with the crate's defaults.
///
/// Bit depth is fixed at 16 in v1. flacenc accepts arbitrary depths via
/// `MemSource::from_samples`, but 16 covers the typical "lossless
/// distribution" case.
pub fn encode_flac(buf: &AudioBuffer, _compression_level: u32) -> AppResult<Vec<u8>> {
    const FLAC_BITS: usize = 16;

    // Convert f32 [-1.0, 1.0] → i32 in the i16 numeric range. flacenc
    // requires samples to be `bits_per_sample`-bounded; for 16 that
    // means [-32768, 32767]. Out-of-range floats are clamped.
    let pcm: Vec<i32> = buf
        .samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i32)
        .collect();

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, err)| AppError::ProcessingFailed {
            detail: format!("flac encode: invalid default config: {err:?}"),
        })?;

    let source = flacenc::source::MemSource::from_samples(
        &pcm,
        usize::from(buf.channels),
        FLAC_BITS,
        buf.sample_rate as usize,
    );

    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("flac encode: {err:?}"),
        })?;

    let mut sink = ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("flac encode (write): {err:?}"),
        })?;
    Ok(sink.as_slice().to_vec())
}

/// Inclusive bitrate bounds for MP3 (kbps). LAME accepts a discrete set
/// (see [`closest_lame_bitrate`]); these are the bounds the UI exposes
/// and we clamp incoming values to. Mirrors the policy on
/// `pdf_to_images`/`image_format_converter`: silently snap to nearest
/// rather than reject.
pub const MP3_BITRATE_MIN: u32 = 96;
pub const MP3_BITRATE_MAX: u32 = 320;

/// The full ladder of LAME-supported CBR bitrates. Indexed for closest-match
/// in [`closest_lame_bitrate`].
const LAME_BITRATE_LADDER: &[(u32, Bitrate)] = &[
    (96, Bitrate::Kbps96),
    (112, Bitrate::Kbps112),
    (128, Bitrate::Kbps128),
    (160, Bitrate::Kbps160),
    (192, Bitrate::Kbps192),
    (224, Bitrate::Kbps224),
    (256, Bitrate::Kbps256),
    (320, Bitrate::Kbps320),
];

/// Clamp `kbps` to `[MP3_BITRATE_MIN, MP3_BITRATE_MAX]` and snap to the
/// nearest LAME-supported rate.
fn closest_lame_bitrate(kbps: u32) -> Bitrate {
    let clamped = kbps.clamp(MP3_BITRATE_MIN, MP3_BITRATE_MAX);
    LAME_BITRATE_LADDER
        .iter()
        .min_by_key(|(k, _)| (i64::from(*k) - i64::from(clamped)).abs())
        .map(|(_, b)| *b)
        .unwrap_or(Bitrate::Kbps192)
}

/// Encode an `AudioBuffer` as MP3 via LAME (`mp3lame-encoder`).
///
/// **Mono / stereo only.** LAME's API tops out at 2 channels; callers
/// must downmix multi-channel sources before getting here. The defensive
/// check is a "shouldn't happen but fail loudly if it does" guard.
///
/// Bitrate is CBR. We snap `kbps` to the nearest LAME-supported rate via
/// [`closest_lame_bitrate`], so callers can pass arbitrary values without
/// the encoder rejecting awkward numbers.
///
/// f32 samples pass through via the IEEE-float overloads of
/// `lame_encode_buffer_*`. LAME expects them in `[-1.0, 1.0]`; we don't
/// clamp here because the decoder paths already produce normalized PCM.
/// If a future decoder excursions out of range, LAME's internal limiter
/// handles it.
pub fn encode_mp3(buf: &AudioBuffer, bitrate_kbps: u32) -> AppResult<Vec<u8>> {
    if buf.channels == 0 || buf.channels > 2 {
        return Err(AppError::ProcessingFailed {
            detail: format!(
                "mp3 encode: LAME supports 1 or 2 channels; buffer has {}. Caller should downmix to mono or stereo first.",
                buf.channels
            ),
        });
    }

    let mut builder = LameBuilder::new().ok_or_else(|| AppError::ProcessingFailed {
        detail: "mp3 encode: failed to allocate LAME builder".into(),
    })?;
    builder
        .set_num_channels(buf.channels as u8)
        .map_err(lame_to_app_err)?;
    builder
        .set_sample_rate(buf.sample_rate)
        .map_err(lame_to_app_err)?;
    builder
        .set_brate(closest_lame_bitrate(bitrate_kbps))
        .map_err(lame_to_app_err)?;
    let mut encoder = builder.build().map_err(lame_to_app_err)?;

    // Output buffer: LAME's `max_required_buffer_size` is sized for the
    // FULL input passed in one go, so we allocate once for the whole
    // input plus a flush tail. Encode is sub-millisecond for tiny inputs
    // and the buffer is freed when this fn returns.
    let frames_per_channel = buf.samples.len() / usize::from(buf.channels);
    // LAME's documented max bytes-per-input-frames, plus a ~7200-byte
    // tail for the final flush (the FlushNoGap path can emit one extra
    // MP3 frame).
    let mut out = Vec::<u8>::with_capacity(
        mp3lame_encoder::max_required_buffer_size(frames_per_channel) + 7200,
    );

    let encoded = match buf.channels {
        1 => encoder
            .encode(MonoPcm(buf.samples.as_slice()), out.spare_capacity_mut())
            .map_err(|err| AppError::ProcessingFailed {
                detail: format!("mp3 encode: {err:?}"),
            })?,
        2 => encoder
            .encode(
                InterleavedPcm(buf.samples.as_slice()),
                out.spare_capacity_mut(),
            )
            .map_err(|err| AppError::ProcessingFailed {
                detail: format!("mp3 encode: {err:?}"),
            })?,
        _ => unreachable!("guarded above"),
    };
    // SAFETY: `encoded` is the number of bytes LAME wrote into
    // `spare_capacity_mut()`; those bytes are now initialized.
    unsafe { out.set_len(out.len() + encoded) };

    let flushed = encoder
        .flush::<FlushNoGap>(out.spare_capacity_mut())
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("mp3 flush: {err:?}"),
        })?;
    // SAFETY: same reasoning — `flushed` bytes written + we own the spare.
    unsafe { out.set_len(out.len() + flushed) };

    Ok(out)
}

fn lame_to_app_err<E: core::fmt::Debug>(err: E) -> AppError {
    AppError::ProcessingFailed {
        detail: format!("mp3 encode (LAME): {err:?}"),
    }
}

/// Inclusive Vorbis quality bounds the UI exposes (Xiph CLI scale,
/// -1.0…10.0). [`xiph_to_internal_quality`] maps to the
/// `VorbisBitrateManagementStrategy::QualityVbr` internal range
/// (`-0.2..=1.0`) that libvorbis actually consumes.
pub const VORBIS_QUALITY_MIN: f32 = -1.0;
pub const VORBIS_QUALITY_MAX: f32 = 10.0;

/// Map the user-facing Xiph CLI quality (`-1..10`, where 5 is the default
/// in `oggenc`) to libvorbis's internal perceptual quality factor
/// (`-0.2..1.0`).
///
/// We use a simple linear remap. The CLI's underlying mapping is
/// non-linear, but for an interactive tool a perceptual "5 ≈ 0.5"
/// linear approximation is close enough — the real subjective difference
/// between adjacent steps is in the bitrate-bracketing logic, not in the
/// quality scalar itself.
fn xiph_to_internal_quality(cli_quality: f32) -> f32 {
    let clamped = cli_quality.clamp(VORBIS_QUALITY_MIN, VORBIS_QUALITY_MAX);
    // -1.0 → -0.2,  10.0 → 1.0,  5.0 → ≈0.5455
    let normalized = (clamped - VORBIS_QUALITY_MIN) / (VORBIS_QUALITY_MAX - VORBIS_QUALITY_MIN);
    -0.2 + normalized * 1.2
}

/// Encode an `AudioBuffer` as Ogg Vorbis via `vorbis_rs`.
///
/// libvorbis natively supports 1..255 channels but the v1 contract here
/// is "what symphonia + claxon decoded"; in practice that's 1 or 2
/// channels. We reject 0-channel inputs and pass anything else straight
/// through to libvorbis, which handles up to 8 reliably and rejects more
/// at the C layer.
///
/// `quality_xiph` is the user-facing Xiph CLI quality (-1..10). We map
/// it to libvorbis's internal `target_quality` in [`xiph_to_internal_quality`].
///
/// libvorbis expects planar (per-channel) input. Callers pass interleaved
/// samples, so we de-interleave into a `Vec<Vec<f32>>` first. Memory cost
/// is fine for v1's "tiny audio file" use cases; streaming-chunked
/// encoding is a follow-up if memory becomes an issue on hour-long inputs.
pub fn encode_ogg_vorbis(buf: &AudioBuffer, quality_xiph: f32) -> AppResult<Vec<u8>> {
    let channels =
        NonZeroU8::new(buf.channels as u8).ok_or_else(|| AppError::ProcessingFailed {
            detail: "ogg encode: zero-channel source".into(),
        })?;
    let sample_rate =
        NonZeroU32::new(buf.sample_rate).ok_or_else(|| AppError::ProcessingFailed {
            detail: "ogg encode: zero sample rate".into(),
        })?;

    // De-interleave the samples into per-channel planes for libvorbis.
    let n_channels = usize::from(buf.channels);
    let frames = buf.samples.len() / n_channels;
    let mut planar: Vec<Vec<f32>> = (0..n_channels)
        .map(|_| Vec::with_capacity(frames))
        .collect();
    for chunk in buf.samples.chunks_exact(n_channels) {
        for (plane, &sample) in planar.iter_mut().zip(chunk.iter()) {
            plane.push(sample);
        }
    }

    let mut sink = Vec::<u8>::new();
    let mut builder =
        VorbisEncoderBuilder::new(sample_rate, channels, &mut sink).map_err(vorbis_to_app_err)?;
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: xiph_to_internal_quality(quality_xiph),
    });
    let mut encoder = builder.build().map_err(vorbis_to_app_err)?;
    encoder
        .encode_audio_block(&planar)
        .map_err(vorbis_to_app_err)?;
    encoder.finish().map_err(vorbis_to_app_err)?;
    Ok(sink)
}

fn vorbis_to_app_err(err: vorbis_rs::VorbisError) -> AppError {
    AppError::ProcessingFailed {
        detail: format!("ogg encode: {err}"),
    }
}

// MaybeUninit is used by mp3lame-encoder's spare_capacity_mut API.
// Re-export here so the imports section reads as documentation of what
// the encoders need.
#[allow(dead_code)]
type _MaybeUninitMarker = MaybeUninit<u8>;

/// LAME's accepted PCM sample rates (Hz). Anything outside this set is
/// rejected by `lame_set_in_samplerate` and must be resampled before
/// encoding — out of scope for v1, which is passthrough-only. Files at
/// other rates are surfaced as a per-file `ProcessingFailed` via
/// [`validate_mp3_sample_rate`]; the caller turns them into skipped-file
/// events for batch tools.
const LAME_SUPPORTED_RATES: &[u32] =
    &[8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000];

/// Reject inputs whose sample rate libmp3lame won't accept. Surfaces as
/// a per-file `ProcessingFailed` with a clear message; callers that batch
/// turn it into a skipped-file event. v1 has no resampler, so this is
/// honest about what gets skipped rather than silently producing garbage.
pub fn validate_mp3_sample_rate(rate: u32) -> AppResult<()> {
    if LAME_SUPPORTED_RATES.contains(&rate) {
        Ok(())
    } else {
        Err(AppError::ProcessingFailed {
            detail: format!(
                "mp3 encode: source sample rate {rate} Hz is not in LAME's accepted set ({:?}); resampling is not implemented in v1",
                LAME_SUPPORTED_RATES
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xiph_to_internal_quality_endpoints() {
        // -1 (CLI min) → -0.2 (libvorbis min);
        // 10 (CLI max) → 1.0 (libvorbis max);
        // 5 → -0.2 + (6/11) * 1.2 ≈ 0.4545 (mid-range under linear remap).
        assert!((xiph_to_internal_quality(-1.0) - -0.2).abs() < 1e-6);
        assert!((xiph_to_internal_quality(10.0) - 1.0).abs() < 1e-6);
        assert!((xiph_to_internal_quality(5.0) - 0.4545455).abs() < 1e-4);
    }

    #[test]
    fn xiph_to_internal_quality_clamps_out_of_range() {
        assert!((xiph_to_internal_quality(-100.0) - -0.2).abs() < 1e-6);
        assert!((xiph_to_internal_quality(100.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn validate_mp3_sample_rate_accepts_44100_and_48000() {
        assert!(validate_mp3_sample_rate(44100).is_ok());
        assert!(validate_mp3_sample_rate(48000).is_ok());
    }

    #[test]
    fn validate_mp3_sample_rate_rejects_96000_with_clear_message() {
        match validate_mp3_sample_rate(96000) {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(
                    detail.contains("96000") && detail.contains("LAME"),
                    "expected detail referencing 96000 + LAME, got {detail}"
                );
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }

    #[test]
    fn lame_supported_rates_set_is_non_empty_and_contains_common_rates() {
        assert!(LAME_SUPPORTED_RATES.contains(&44100));
        assert!(LAME_SUPPORTED_RATES.contains(&48000));
        assert!(!LAME_SUPPORTED_RATES.is_empty());
    }
}
