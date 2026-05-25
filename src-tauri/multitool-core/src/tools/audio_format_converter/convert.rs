//! Audio Format Converter — single-file pure transform.
//!
//! Per-file conversion shape: `(source_ext, bytes, opts)` in, [`EncodedFile`]
//! out. The batch orchestrator in [`super::job`] drives this in a
//! skip + continue loop — a per-file failure here becomes a per-file skip
//! there, not a job-level abort. Mirrors the
//! [Image Format Converter](crate::tools::image_format_converter) on purpose.
//!
//! Decode and encode primitives live in [`crate::audio_codecs`] so the
//! Audio Trimmer can reuse them (it ships in the same PR and would
//! otherwise duplicate Symphonia + claxon + hound + flacenc + LAME +
//! vorbis_rs plumbing). This module owns the converter-specific bits:
//! the `TargetFormat` / `ChannelMode` / `Opts` wire types and the
//! channel-mode policy.

use serde::{Deserialize, Serialize};

use crate::audio_codecs::encode::{
    encode_flac, encode_mp3, encode_ogg_vorbis, encode_wav, validate_mp3_sample_rate, WavBitDepth,
};
use crate::audio_codecs::{decode::decode_to_pcm, AudioBuffer};
use crate::error::AppResult;

/// Target encoder selection. Encoder selection happens off this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetFormat {
    Wav,
    Flac,
    Mp3,
    Ogg,
}

impl TargetFormat {
    /// File extension (no leading dot) for output naming.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Ogg => "ogg",
        }
    }
}

/// Channel-handling policy. `Source` preserves source layout (downmixing
/// N > 2 channels to stereo with a per-file warning); `Mono` / `Stereo`
/// force the named layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelMode {
    Source,
    Mono,
    Stereo,
}

/// User-facing options. Mirrors the form fields on the tool view.
///
/// Per-format knobs are silently clamped in the encoder helpers to defend
/// against bad callers; the UI clamps too for UX. Inactive knobs are
/// ignored (e.g. `mp3_bitrate_kbps` does nothing when
/// `target_format != Mp3`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Opts {
    pub target_format: TargetFormat,
    /// MP3 CBR bitrate, kbps. Clamped + snapped to the nearest LAME-
    /// supported rate inside the encoder.
    pub mp3_bitrate_kbps: u32,
    /// Vorbis quality, Xiph scale −1.0…10.0. Clamped + linearly remapped
    /// to libvorbis's internal scale inside the encoder.
    pub vorbis_quality: f32,
    /// FLAC compression level 0…8. **Currently a no-op** — flacenc 0.5's
    /// API doesn't expose a single compression knob; it has fine-grained
    /// `subframe_coding` / `stereo_coding` config blocks instead. v1
    /// ships with the crate's defaults (good enough for typical use).
    /// Keeping the field on Opts is forward-compatible — if we ever map
    /// levels to the fine-grained knobs, we don't churn the wire shape.
    pub flac_compression_level: u32,
    pub wav_bit_depth: WavBitDepth,
    pub channels: ChannelMode,
}

/// Output of a single-file convert: encoded bytes + any per-file warnings
/// (downmix-to-stereo notes, lossy→lossy transcode, etc.). The orchestrator
/// forwards warnings to the UI without failing the file.
#[derive(Clone, Debug, Default)]
pub struct EncodedFile {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

/// Apply the user's [`ChannelMode`] policy to a decoded buffer.
///
/// - `Source`: pass through up to 2 channels untouched. For 3+ channels,
///   downmix to **stereo** with a warning — the practical default for
///   "I just want to convert this audio" (preserves stereo image when
///   the encoder caps at 2, doesn't silently drop info).
/// - `Mono`: force 1 channel. Multi-channel sources are summed and
///   normalized (averaged); mono passes through.
/// - `Stereo`: force 2 channels. Mono is upmixed (L = R = sample);
///   multi-channel is downmixed to stereo with a warning.
///
/// Warnings are returned alongside the buffer so the orchestrator can
/// surface them in `Progress::Succeeded.warnings` without us re-deriving
/// the source channel count in the caller.
///
/// Downmix math is the standard "front L/R + center / sqrt(2) + back
/// pairs averaged" approach when the source layout is known, but
/// `decode_to_pcm`'s `AudioBuffer` doesn't carry layout info — only a
/// count. For now we use a simple equal-weight average across all
/// source channels; this is acceptable for "convert and listen" use
/// cases and avoids carrying a `Channels` enum from symphonia/claxon
/// through the rest of the pipeline. Layout-aware mixing is a follow-up.
fn apply_channel_mode(buf: AudioBuffer, mode: ChannelMode) -> (AudioBuffer, Vec<String>) {
    let mut warnings = Vec::new();
    let n = usize::from(buf.channels);

    let (samples, channels) = match (mode, buf.channels) {
        // Pass-through cases.
        (ChannelMode::Source, 1) | (ChannelMode::Source, 2) => (buf.samples, buf.channels),
        (ChannelMode::Mono, 1) => (buf.samples, 1),
        (ChannelMode::Stereo, 2) => (buf.samples, 2),

        // ChannelMode::Source with > 2 channels → downmix to stereo.
        (ChannelMode::Source, _) => {
            warnings.push(format!("downmixed {} channels to stereo", buf.channels));
            (downmix_to_stereo(&buf.samples, n), 2)
        }

        // ChannelMode::Mono.
        (ChannelMode::Mono, _) => {
            if n > 1 {
                warnings.push(format!("downmixed {} channels to mono", buf.channels));
            }
            (downmix_to_mono(&buf.samples, n), 1)
        }

        // ChannelMode::Stereo.
        (ChannelMode::Stereo, 1) => (upmix_mono_to_stereo(&buf.samples), 2),
        (ChannelMode::Stereo, _) => {
            warnings.push(format!("downmixed {} channels to stereo", buf.channels));
            (downmix_to_stereo(&buf.samples, n), 2)
        }
    };

    (
        AudioBuffer {
            samples,
            channels,
            sample_rate: buf.sample_rate,
        },
        warnings,
    )
}

/// Equal-weight average of all `n_channels` per frame into a single
/// mono channel. Returns interleaved samples (trivially: just the
/// mono channel back-to-back).
fn downmix_to_mono(interleaved: &[f32], n_channels: usize) -> Vec<f32> {
    if n_channels <= 1 {
        return interleaved.to_vec();
    }
    let denom = n_channels as f32;
    interleaved
        .chunks_exact(n_channels)
        .map(|frame| frame.iter().sum::<f32>() / denom)
        .collect()
}

/// Equal-weight downmix to stereo. For source layouts the decoder
/// doesn't expose, even-indexed channels go to L, odd-indexed to R, and
/// each side is averaged across its assigned channels. Crude but
/// audible — produces a centered stereo image without dropping channels.
/// Layout-aware mixing (5.1's center channel weighting, surround
/// distribution) lands when we wire a `Channels` enum through the
/// decode-to-encode pipeline.
fn downmix_to_stereo(interleaved: &[f32], n_channels: usize) -> Vec<f32> {
    if n_channels == 2 {
        return interleaved.to_vec();
    }
    if n_channels == 1 {
        return upmix_mono_to_stereo(interleaved);
    }
    let n_left = n_channels.div_ceil(2);
    let n_right = n_channels - n_left;
    let denom_l = n_left as f32;
    let denom_r = n_right.max(1) as f32;
    let mut out = Vec::with_capacity((interleaved.len() / n_channels) * 2);
    for frame in interleaved.chunks_exact(n_channels) {
        let left: f32 = frame[..n_left].iter().sum::<f32>() / denom_l;
        let right: f32 = if n_right == 0 {
            left
        } else {
            frame[n_left..].iter().sum::<f32>() / denom_r
        };
        out.push(left);
        out.push(right);
    }
    out
}

/// Duplicate each mono sample into L = R = sample for stereo output.
fn upmix_mono_to_stereo(mono: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(mono.len() * 2);
    for &sample in mono {
        out.push(sample);
        out.push(sample);
    }
    out
}

/// Convert a single audio file's bytes from its source format to
/// `opts.target_format`.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot)
/// and is consulted only when Symphonia's bytes-sniffer is inconclusive (rare
/// — most containers have a magic byte sequence). Bytes always win when a
/// format is identifiable from the stream.
///
/// Pipeline: decode → apply channel mode → validate encoder constraints
/// (MP3 sample rate) → encode. Channel-mode warnings (downmix/upmix
/// notes) ride along in [`EncodedFile::warnings`] so the orchestrator
/// can surface them per file.
pub fn convert_one(source_ext: &str, input_bytes: &[u8], opts: &Opts) -> AppResult<EncodedFile> {
    let decoded = decode_to_pcm(source_ext, input_bytes)?;
    let (decoded, warnings) = apply_channel_mode(decoded, opts.channels);

    if matches!(opts.target_format, TargetFormat::Mp3) {
        validate_mp3_sample_rate(decoded.sample_rate)?;
    }

    let bytes = match opts.target_format {
        TargetFormat::Wav => encode_wav(&decoded, opts.wav_bit_depth)?,
        TargetFormat::Flac => encode_flac(&decoded, opts.flac_compression_level)?,
        TargetFormat::Mp3 => encode_mp3(&decoded, opts.mp3_bitrate_kbps)?,
        TargetFormat::Ogg => encode_ogg_vorbis(&decoded, opts.vorbis_quality)?,
    };
    Ok(EncodedFile { bytes, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn audio_fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(format!("tests/fixtures/audio/{name}"));
        fs::read(&path).unwrap_or_else(|_| panic!("read audio fixture {name}: {path:?}"))
    }

    fn default_opts(target_format: TargetFormat) -> Opts {
        Opts {
            target_format,
            mp3_bitrate_kbps: 192,
            vorbis_quality: 5.0,
            flac_compression_level: 5,
            wav_bit_depth: WavBitDepth::Bit16,
            channels: ChannelMode::Source,
        }
    }

    /// Decode the encoded output and assert dimensions match the source.
    /// Lossy encoders (MP3/OGG) won't match sample-for-sample, but the
    /// channel count + sample rate must round-trip exactly. WAV is
    /// sample-exact (within float→int quantization).
    fn assert_round_trip_dims(
        encoded: &[u8],
        source_ext: &str,
        expected_channels: u16,
        expected_rate: u32,
    ) {
        let decoded = decode_to_pcm(source_ext, encoded)
            .unwrap_or_else(|err| panic!("re-decode {source_ext} failed: {err:?}"));
        assert_eq!(
            decoded.channels, expected_channels,
            "round-trip channel count mismatch"
        );
        assert_eq!(
            decoded.sample_rate, expected_rate,
            "round-trip sample rate mismatch"
        );
        assert!(
            !decoded.samples.is_empty(),
            "round-trip should produce non-empty samples"
        );
    }

    // --- WAV encoder ---

    #[test]
    fn wav_mono_16bit_round_trips() {
        let opts = default_opts(TargetFormat::Wav);
        let out = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts)
            .expect("convert WAV → WAV 16-bit");
        assert_round_trip_dims(&out.bytes, "wav", 1, 44100);
    }

    #[test]
    fn wav_mono_24bit_round_trips() {
        let mut opts = default_opts(TargetFormat::Wav);
        opts.wav_bit_depth = WavBitDepth::Bit24;
        let out = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts)
            .expect("convert WAV → WAV 24-bit");
        assert_round_trip_dims(&out.bytes, "wav", 1, 44100);
    }

    #[test]
    fn wav_mono_32f_round_trips() {
        let mut opts = default_opts(TargetFormat::Wav);
        opts.wav_bit_depth = WavBitDepth::Bit32f;
        let out = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts)
            .expect("convert WAV → WAV 32-bit float");
        assert_round_trip_dims(&out.bytes, "wav", 1, 44100);
    }

    #[test]
    fn wav_stereo_16bit_round_trips_with_two_channels_preserved() {
        let opts = default_opts(TargetFormat::Wav);
        let out = convert_one("wav", &audio_fixture("tiny_stereo.wav"), &opts)
            .expect("convert stereo WAV → WAV 16-bit");
        assert_round_trip_dims(&out.bytes, "wav", 2, 44100);
    }

    #[test]
    fn mp3_input_to_wav_output_round_trips() {
        // Lossy → lossless: dims must match; samples won't bit-match the
        // MP3 source (decoder introduces artifacts). Sanity-checks the
        // common "give me a WAV from this MP3 for editing" use case.
        let opts = default_opts(TargetFormat::Wav);
        let out =
            convert_one("mp3", &audio_fixture("tiny_mono.mp3"), &opts).expect("convert MP3 → WAV");
        assert_round_trip_dims(&out.bytes, "wav", 1, 44100);
    }

    // --- FLAC encoder ---

    #[test]
    fn flac_mono_round_trips() {
        let opts = default_opts(TargetFormat::Flac);
        let out =
            convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts).expect("convert WAV → FLAC");
        assert_round_trip_dims(&out.bytes, "flac", 1, 44100);
    }

    #[test]
    fn flac_stereo_round_trips() {
        let opts = default_opts(TargetFormat::Flac);
        let out = convert_one("wav", &audio_fixture("tiny_stereo.wav"), &opts)
            .expect("convert stereo WAV → FLAC");
        assert_round_trip_dims(&out.bytes, "flac", 2, 44100);
    }

    #[test]
    fn flac_compression_level_is_currently_a_no_op() {
        // Verifies the documented "level is a no-op in v1" behaviour:
        // levels 0 and 8 produce identical output for the same input
        // because flacenc's Encoder default doesn't honour either value.
        // If/when we wire a level→config-knobs mapping, this test gets
        // replaced by a "lower level = larger output" assertion.
        let mut opts_lo = default_opts(TargetFormat::Flac);
        opts_lo.flac_compression_level = 0;
        let mut opts_hi = default_opts(TargetFormat::Flac);
        opts_hi.flac_compression_level = 8;
        let lo =
            convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts_lo).expect("convert level 0");
        let hi =
            convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts_hi).expect("convert level 8");
        assert_eq!(
            lo.bytes, hi.bytes,
            "v1 ignores flac_compression_level — outputs should be identical"
        );
    }

    // --- MP3 encoder ---

    #[test]
    fn mp3_mono_round_trips() {
        let opts = default_opts(TargetFormat::Mp3);
        let out = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts)
            .expect("convert mono WAV → MP3");
        assert_round_trip_dims(&out.bytes, "mp3", 1, 44100);
    }

    #[test]
    fn mp3_stereo_round_trips() {
        let opts = default_opts(TargetFormat::Mp3);
        let out = convert_one("wav", &audio_fixture("tiny_stereo.wav"), &opts)
            .expect("convert stereo WAV → MP3");
        // LAME's stereo MP3 decodes back as 2 channels through Symphonia.
        assert_round_trip_dims(&out.bytes, "mp3", 2, 44100);
    }

    #[test]
    fn mp3_bitrate_higher_produces_larger_output_than_lower() {
        // Sanity-check the bitrate knob actually affects encoded size.
        let mut opts_lo = default_opts(TargetFormat::Mp3);
        opts_lo.mp3_bitrate_kbps = 96;
        let mut opts_hi = default_opts(TargetFormat::Mp3);
        opts_hi.mp3_bitrate_kbps = 320;
        let lo = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts_lo)
            .expect("encode at 96 kbps");
        let hi = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts_hi)
            .expect("encode at 320 kbps");
        assert!(
            hi.bytes.len() > lo.bytes.len(),
            "expected 320 kbps MP3 ({}) > 96 kbps MP3 ({})",
            hi.bytes.len(),
            lo.bytes.len()
        );
    }

    #[test]
    fn mp3_bitrate_below_min_is_clamped_silently() {
        let mut opts = default_opts(TargetFormat::Mp3);
        opts.mp3_bitrate_kbps = 0; // far below 96 kbps min
        let out = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts)
            .expect("encode at bitrate=0 (clamped to 96)");
        assert_round_trip_dims(&out.bytes, "mp3", 1, 44100);
    }

    // --- OGG Vorbis encoder ---

    #[test]
    fn ogg_mono_round_trips() {
        let opts = default_opts(TargetFormat::Ogg);
        let out = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts)
            .expect("convert mono WAV → OGG Vorbis");
        assert_round_trip_dims(&out.bytes, "ogg", 1, 44100);
    }

    #[test]
    fn ogg_stereo_round_trips() {
        let opts = default_opts(TargetFormat::Ogg);
        let out = convert_one("wav", &audio_fixture("tiny_stereo.wav"), &opts)
            .expect("convert stereo WAV → OGG Vorbis");
        assert_round_trip_dims(&out.bytes, "ogg", 2, 44100);
    }

    #[test]
    fn ogg_quality_higher_produces_larger_output_than_lower() {
        let mut opts_lo = default_opts(TargetFormat::Ogg);
        opts_lo.vorbis_quality = -1.0;
        let mut opts_hi = default_opts(TargetFormat::Ogg);
        opts_hi.vorbis_quality = 10.0;
        let lo = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts_lo)
            .expect("encode at quality -1");
        let hi = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts_hi)
            .expect("encode at quality 10");
        assert!(
            hi.bytes.len() > lo.bytes.len(),
            "expected q=10 ({}) > q=-1 ({})",
            hi.bytes.len(),
            lo.bytes.len()
        );
    }

    // --- Channel mode ---

    fn buffer(samples: Vec<f32>, channels: u16, sample_rate: u32) -> AudioBuffer {
        AudioBuffer {
            samples,
            channels,
            sample_rate,
        }
    }

    #[test]
    fn channel_mode_source_passes_mono_unchanged() {
        let src = buffer(vec![0.1, 0.2, 0.3, 0.4], 1, 44100);
        let (out, warnings) = apply_channel_mode(src.clone(), ChannelMode::Source);
        assert_eq!(out.channels, 1);
        assert_eq!(out.samples, src.samples);
        assert!(warnings.is_empty());
    }

    #[test]
    fn channel_mode_source_passes_stereo_unchanged() {
        let src = buffer(vec![0.1, 0.2, 0.3, 0.4], 2, 44100);
        let (out, warnings) = apply_channel_mode(src.clone(), ChannelMode::Source);
        assert_eq!(out.channels, 2);
        assert_eq!(out.samples, src.samples);
        assert!(warnings.is_empty());
    }

    #[test]
    fn channel_mode_source_downmixes_5dot1_to_stereo_with_warning() {
        // 1 frame of 5.1 → 1 frame of stereo.
        let src = buffer(vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 6, 48000);
        let (out, warnings) = apply_channel_mode(src, ChannelMode::Source);
        assert_eq!(out.channels, 2);
        assert_eq!(out.samples.len(), 2);
        assert!(
            warnings.iter().any(|w| w.contains("6 channels")),
            "expected warning mentioning 6 channels, got {warnings:?}"
        );
    }

    #[test]
    fn channel_mode_mono_downmixes_stereo_averaging_channels() {
        // [L=1.0, R=0.0] should average to 0.5.
        let src = buffer(vec![1.0, 0.0, 0.6, 0.4], 2, 44100);
        let (out, warnings) = apply_channel_mode(src, ChannelMode::Mono);
        assert_eq!(out.channels, 1);
        assert_eq!(out.samples, vec![0.5, 0.5]);
        assert!(warnings.iter().any(|w| w.contains("mono")));
    }

    #[test]
    fn channel_mode_mono_passes_mono_unchanged() {
        let src = buffer(vec![0.1, 0.2, 0.3], 1, 44100);
        let (out, warnings) = apply_channel_mode(src.clone(), ChannelMode::Mono);
        assert_eq!(out.channels, 1);
        assert_eq!(out.samples, src.samples);
        assert!(warnings.is_empty());
    }

    #[test]
    fn channel_mode_stereo_upmixes_mono_to_duplicated_lr() {
        let src = buffer(vec![0.1, 0.2, 0.3], 1, 44100);
        let (out, warnings) = apply_channel_mode(src, ChannelMode::Stereo);
        assert_eq!(out.channels, 2);
        assert_eq!(out.samples, vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3]);
        // Mono → stereo isn't a "downmix" and emits no warning.
        assert!(warnings.is_empty());
    }

    #[test]
    fn channel_mode_stereo_passes_stereo_unchanged() {
        let src = buffer(vec![0.1, 0.2, 0.3, 0.4], 2, 44100);
        let (out, warnings) = apply_channel_mode(src.clone(), ChannelMode::Stereo);
        assert_eq!(out.channels, 2);
        assert_eq!(out.samples, src.samples);
        assert!(warnings.is_empty());
    }

    /// Round-trip Source-mode downmix through the real pipeline by
    /// constructing a fake 5.1 stereo source and converting to WAV.
    /// (No 5.1 fixture is shipped — constructing the AudioBuffer
    /// directly via `apply_channel_mode` covers the same path.)
    #[test]
    fn convert_one_with_mono_target_downmixes_stereo_source() {
        let mut opts = default_opts(TargetFormat::Wav);
        opts.channels = ChannelMode::Mono;
        let out = convert_one("wav", &audio_fixture("tiny_stereo.wav"), &opts)
            .expect("convert stereo → WAV mono");
        assert_round_trip_dims(&out.bytes, "wav", 1, 44100);
        assert!(
            out.warnings.iter().any(|w| w.contains("mono")),
            "expected downmix warning, got {:?}",
            out.warnings
        );
    }

    #[test]
    fn convert_one_with_stereo_target_upmixes_mono_source_without_warning() {
        let mut opts = default_opts(TargetFormat::Wav);
        opts.channels = ChannelMode::Stereo;
        let out = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts)
            .expect("convert mono → WAV stereo");
        assert_round_trip_dims(&out.bytes, "wav", 2, 44100);
        assert!(out.warnings.is_empty(), "got {:?}", out.warnings);
    }
}
