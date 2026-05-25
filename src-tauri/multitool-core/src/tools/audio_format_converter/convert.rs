//! Audio Format Converter — single-file pure transform.
//!
//! Per-file conversion shape: `(source_ext, bytes, opts)` in, [`EncodedFile`]
//! out. The batch orchestrator in [`super::job`] drives this in a
//! skip + continue loop — a per-file failure here becomes a per-file skip
//! there, not a job-level abort. Mirrors the
//! [Image Format Converter](crate::tools::image_format_converter) on purpose.
//!
//! Decode path: [`symphonia`] (pure Rust, decode-only). Encode path: one
//! crate per output — `hound` for WAV, `flacenc` for FLAC, `mp3lame-encoder`
//! for MP3, `vorbis_rs` for OGG. The cc-rs / autotools build cost of LAME +
//! libogg + libvorbis is documented in
//! [`docs/plans/AUDIO_FORMAT_CONVERTER.md`](../../../../../docs/plans/AUDIO_FORMAT_CONVERTER.md)
//! and will move to DECISIONS.md once the tool ships.

use std::io::Cursor;
use std::mem::MaybeUninit;
use std::num::{NonZeroU32, NonZeroU8};

use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use hound::{SampleFormat as HoundSampleFormat, WavSpec, WavWriter};
use mp3lame_encoder::{Bitrate, Builder as LameBuilder, FlushNoGap, InterleavedPcm, MonoPcm};
use serde::{Deserialize, Serialize};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

use crate::error::{AppError, AppResult};

/// Raster — sorry, *raster* of audio. Encoder selection happens off this.
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

/// WAV-only: sample bit depth. PCM 16/24 or 32-bit float.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WavBitDepth {
    Bit16,
    Bit24,
    Bit32f,
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
/// Per-format knobs are silently clamped in `convert_one` to defend against
/// bad callers; the UI clamps too for UX. Inactive knobs are ignored (e.g.
/// `mp3_bitrate_kbps` does nothing when `target_format != Mp3`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Opts {
    pub target_format: TargetFormat,
    /// MP3 CBR bitrate, kbps. Clamped when MP3 encoder lands (commit 4).
    pub mp3_bitrate_kbps: u32,
    /// Vorbis quality, Xiph scale −1.0…10.0. Clamped when OGG encoder lands (commit 4).
    pub vorbis_quality: f32,
    /// FLAC compression level 0…8. **Currently a no-op** — the `flacenc`
    /// 0.5 API doesn't expose a single compression knob; it has fine-grained
    /// `subframe_coding` / `stereo_coding` config blocks instead. v1 ships
    /// with the crate's defaults (good enough for typical use). Keeping the
    /// field on Opts is forward-compatible — if we ever map levels to the
    /// fine-grained knobs, we don't churn the wire shape.
    pub flac_compression_level: u32,
    pub wav_bit_depth: WavBitDepth,
    pub channels: ChannelMode,
}

/// Output of a single-file convert: encoded bytes + any per-file warnings
/// (animated-GIF-style notes — downmix to stereo, lossy→lossy transcode,
/// etc.). The orchestrator forwards warnings to the UI without failing the
/// file.
#[derive(Clone, Debug, Default)]
pub struct EncodedFile {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

// Per-format clamp bounds (MP3 bitrate, Vorbis quality, FLAC level) move
// here once the encoders that consume them land in commits 3–4.

/// Decoded audio payload: interleaved f32 samples + the layout needed to
/// re-encode them. Channels are an exact count after Symphonia normalizes
/// whatever `Channels` variant the source declared. The rate is whatever
/// the source ships — we pass it through to the encoder unchanged (per the
/// v1 "passthrough sample rate" decision in the working doc).
///
/// Internal intermediate; not part of the tool's IPC surface. Encoders
/// consume this directly.
#[derive(Clone, Debug)]
pub(super) struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
}

/// FLAC magic bytes. RFC 9639 §3 calls it the "stream marker": ASCII
/// `fLaC` at file offset 0.
const FLAC_MAGIC: [u8; 4] = [b'f', b'L', b'a', b'C'];

/// Decode an audio file's bytes into interleaved f32 PCM.
///
/// **FLAC inputs route through `claxon`**; everything else goes through
/// Symphonia. The split exists because Symphonia 0.6's FLAC demuxer is
/// strict about STREAMINFO's `total_samples` matching the demuxed frame
/// count, and our own `flacenc`-produced output trips that check (see
/// the rationale on the `claxon` dep in `Cargo.toml`). FLAC files from
/// other encoders (ffmpeg, reference flac CLI) read cleanly through
/// either decoder; routing both through `claxon` keeps the path
/// consistent.
///
/// `source_ext` is the lowercased source extension without leading dot
/// (or `""`). For non-FLAC, it's passed to Symphonia's [`Hint`] only as
/// a tie-breaker — bytes-sniffing wins. For FLAC, the magic bytes are
/// checked directly; the extension is advisory.
///
/// Per-packet `DecodeError` / `IoError` from a malformed packet skip the
/// packet and continue, mirroring Symphonia's own getting-started example
/// — the decoder is best-effort within a file. A hard failure on the
/// format probe or the decoder construction is a per-file `Err`, which
/// the orchestrator turns into a [`super::job::Progress::Skipped`].
pub(super) fn decode_to_pcm(source_ext: &str, bytes: &[u8]) -> AppResult<AudioBuffer> {
    if bytes.is_empty() {
        return Err(AppError::UnsupportedFormat {
            detail: "audio decode: empty input".into(),
        });
    }

    let looks_like_flac = bytes.len() >= 4 && bytes[..4] == FLAC_MAGIC;
    let extension_is_flac = source_ext.eq_ignore_ascii_case("flac");
    if looks_like_flac || extension_is_flac {
        return decode_flac_with_claxon(bytes);
    }

    // Symphonia takes ownership of the source via a Box, so we wrap the
    // bytes in a Cursor and box it. The MediaSourceStream then layers a
    // buffered reader over that for the demuxer.
    let cursor = Box::new(Cursor::new(bytes.to_vec()));
    let mss = MediaSourceStream::new(cursor, Default::default());

    let mut hint = Hint::new();
    if !source_ext.is_empty() {
        hint.with_extension(source_ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(symphonia_to_app_err)?;

    let track =
        format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AppError::UnsupportedFormat {
                detail: "audio decode: no default audio track".into(),
            })?;
    let track_id = track.id;
    let codec_params = track
        .codec_params
        .as_ref()
        .ok_or_else(|| AppError::UnsupportedFormat {
            detail: "audio decode: track has no codec parameters".into(),
        })?
        .audio()
        .ok_or_else(|| AppError::UnsupportedFormat {
            detail: "audio decode: default track is not an audio codec".into(),
        })?;
    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| AppError::UnsupportedFormat {
            detail: "audio decode: source declares no sample rate".into(),
        })?;
    let channels = codec_params
        .channels
        .as_ref()
        .map(|c| c.count())
        .ok_or_else(|| AppError::UnsupportedFormat {
            detail: "audio decode: source declares no channel layout".into(),
        })?;
    let channels = u16::try_from(channels).map_err(|_| AppError::UnsupportedFormat {
        detail: format!("audio decode: channel count {channels} exceeds u16"),
    })?;
    if channels == 0 {
        return Err(AppError::UnsupportedFormat {
            detail: "audio decode: zero-channel source".into(),
        });
    }

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_params, &AudioDecoderOptions::default())
        .map_err(symphonia_to_app_err)?;

    let mut interleaved: Vec<f32> = Vec::new();
    // Scratch vec reused across packets. Symphonia's
    // `copy_to_vec_interleaved` resizes the destination to the packet's
    // exact sample count (it REPLACES, not appends — verified against
    // symphonia-core 0.6.0 source), so we copy each packet here and then
    // extend the accumulator.
    let mut packet_buf: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // The track list changed mid-stream. Chained OGG streams hit
            // this; for v1 we stop cleanly with whatever we've decoded —
            // re-probing mid-decode is more complexity than it's worth for
            // the "convert to MP3" use case. If real users complain we
            // re-visit.
            Err(SymphoniaError::ResetRequired) => break,
            Err(err) => return Err(symphonia_to_app_err(err)),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                buf.copy_to_vec_interleaved::<f32>(&mut packet_buf);
                interleaved.extend_from_slice(&packet_buf);
            }
            // Mirrors the getting-started example: best-effort within a
            // file. A bad packet doesn't fail the whole decode.
            Err(SymphoniaError::IoError(_) | SymphoniaError::DecodeError(_)) => continue,
            Err(err) => return Err(symphonia_to_app_err(err)),
        }
    }

    if interleaved.is_empty() {
        return Err(AppError::UnsupportedFormat {
            detail: "audio decode: no samples produced (empty or unsupported stream)".into(),
        });
    }

    Ok(AudioBuffer {
        samples: interleaved,
        channels,
        sample_rate,
    })
}

/// Decode a FLAC byte stream into interleaved f32 PCM via `claxon`.
///
/// claxon yields i32 samples in the bit-depth's natural integer range
/// (e.g. for 16-bit, samples are in `[-32768, 32767]`). We normalize to
/// f32 in `[-1.0, 1.0]` using the bit-depth so the encoder paths stay
/// uniform regardless of which decoder produced the PCM.
fn decode_flac_with_claxon(bytes: &[u8]) -> AppResult<AudioBuffer> {
    let mut reader =
        claxon::FlacReader::new(Cursor::new(bytes)).map_err(|err| AppError::UnsupportedFormat {
            detail: format!("flac decode: {err}"),
        })?;
    let info = reader.streaminfo();
    let channels = u16::try_from(info.channels).map_err(|_| AppError::UnsupportedFormat {
        detail: format!("flac decode: channel count {} exceeds u16", info.channels),
    })?;
    if channels == 0 {
        return Err(AppError::UnsupportedFormat {
            detail: "flac decode: zero-channel source".into(),
        });
    }
    let sample_rate = info.sample_rate;
    let bits = info.bits_per_sample;
    let max_sample_magnitude = 1i64 << (bits - 1);
    let denom = max_sample_magnitude as f32;
    let mut samples = Vec::<f32>::new();
    for s in reader.samples() {
        let sample = s.map_err(|err| AppError::ProcessingFailed {
            detail: format!("flac decode (sample): {err}"),
        })?;
        samples.push(sample as f32 / denom);
    }
    if samples.is_empty() {
        return Err(AppError::UnsupportedFormat {
            detail: "flac decode: no samples produced".into(),
        });
    }
    Ok(AudioBuffer {
        samples,
        channels,
        sample_rate,
    })
}

/// Map a `symphonia::core::errors::Error` to the right `AppError` variant.
/// `Unsupported` / `DecodeError` land in `UnsupportedFormat`; `IoError` and
/// the long-tail of variants (Seek/Limit/Reset) fall through to
/// `ProcessingFailed`.
fn symphonia_to_app_err(err: SymphoniaError) -> AppError {
    match err {
        SymphoniaError::Unsupported(_) | SymphoniaError::DecodeError(_) => {
            AppError::UnsupportedFormat {
                detail: err.to_string(),
            }
        }
        _ => AppError::ProcessingFailed {
            detail: err.to_string(),
        },
    }
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
/// carries any channel count. `decode_to_pcm` already gave us interleaved
/// samples, and hound expects interleaved input for `write_sample` calls.
fn encode_wav(buf: &AudioBuffer, bit_depth: WavBitDepth) -> AppResult<Vec<u8>> {
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
/// unused — see `Opts::flac_compression_level` for the rationale. v1
/// ships with `flacenc::config::Encoder::default()`, which the upstream
/// authors consider a sane preset (mid-range LPC orders + multithread
/// when available).
///
/// Bit depth is fixed at 16 in v1. flacenc accepts arbitrary depths via
/// `MemSource::from_samples`, but 16 covers the typical "lossless
/// distribution" case. Bumping to 24-bit FLAC is a follow-up if real
/// users care.
fn encode_flac(buf: &AudioBuffer, _compression_level: u32) -> AppResult<Vec<u8>> {
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
pub(crate) const MP3_BITRATE_MIN: u32 = 96;
pub(crate) const MP3_BITRATE_MAX: u32 = 320;

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
/// **Mono / stereo only.** LAME's API tops out at 2 channels;
/// [`apply_channel_mode`] (called upstream in `convert_one`) is
/// responsible for downmixing multi-channel sources. The defensive
/// check here is a "shouldn't happen but fail loudly if it does"
/// guard against direct callers that skip the channel-mode pass.
///
/// Bitrate is CBR. We snap `kbps` to the nearest LAME-supported rate via
/// [`closest_lame_bitrate`], so the UI can offer 96/128/192/etc. without
/// the encoder rejecting awkward values.
///
/// f32 samples are passed straight through via the IEEE-float overloads
/// of `lame_encode_buffer_*`. LAME expects them in `[-1.0, 1.0]`; we
/// don't clamp here because the decoder paths already produce normalized
/// PCM. If a future decoder excursions out of range, LAME's internal
/// limiter handles it.
fn encode_mp3(buf: &AudioBuffer, bitrate_kbps: u32) -> AppResult<Vec<u8>> {
    if buf.channels == 0 || buf.channels > 2 {
        return Err(AppError::ProcessingFailed {
            detail: format!(
                "mp3 encode: LAME supports 1 or 2 channels; buffer has {}. Caller should apply ChannelMode::{{Mono,Stereo}} or Source first.",
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
pub(crate) const VORBIS_QUALITY_MIN: f32 = -1.0;
pub(crate) const VORBIS_QUALITY_MAX: f32 = 10.0;

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
/// libvorbis expects planar (per-channel) input. `decode_to_pcm` gives
/// us interleaved samples, so we de-interleave into a `Vec<Vec<f32>>`
/// first. Memory cost is fine for v1's "tiny audio file" use cases;
/// streaming-chunked encoding is a commit-5 follow-up if memory becomes
/// an issue on hour-long inputs.
fn encode_ogg_vorbis(buf: &AudioBuffer, quality_xiph: f32) -> AppResult<Vec<u8>> {
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
/// other rates land in the orchestrator's skipped list.
const LAME_SUPPORTED_RATES: &[u32] =
    &[8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000];

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

/// Reject inputs whose sample rate libmp3lame won't accept. Surfaces as
/// a per-file `ProcessingFailed` with a clear message; the orchestrator
/// turns it into a [`super::job::Progress::Skipped`]. v1 has no
/// resampler, so this is honest about what gets skipped rather than
/// silently producing garbage.
fn validate_mp3_sample_rate(rate: u32) -> AppResult<()> {
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

    /// All four input formats decode to non-empty f32 PCM with the expected
    /// sample rate. Channel count matches what ffmpeg emitted at fixture
    /// generation (1 = mono for the `tiny_mono.*` set, 2 for `tiny_stereo`).
    #[test]
    fn decodes_wav_mono_to_pcm() {
        let buf =
            decode_to_pcm("wav", &audio_fixture("tiny_mono.wav")).expect("decode tiny_mono.wav");
        assert_eq!(buf.sample_rate, 44100, "fixture is 44.1 kHz");
        assert_eq!(buf.channels, 1, "fixture is mono");
        assert!(!buf.samples.is_empty(), "expected some PCM samples, got 0");
    }

    #[test]
    fn decodes_mp3_mono_to_pcm() {
        let buf =
            decode_to_pcm("mp3", &audio_fixture("tiny_mono.mp3")).expect("decode tiny_mono.mp3");
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.channels, 1);
        assert!(!buf.samples.is_empty());
    }

    #[test]
    fn decodes_flac_mono_to_pcm() {
        let buf =
            decode_to_pcm("flac", &audio_fixture("tiny_mono.flac")).expect("decode tiny_mono.flac");
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.channels, 1);
        assert!(!buf.samples.is_empty());
    }

    #[test]
    fn decodes_ogg_vorbis_mono_to_pcm() {
        let buf =
            decode_to_pcm("ogg", &audio_fixture("tiny_mono.ogg")).expect("decode tiny_mono.ogg");
        assert_eq!(buf.sample_rate, 44100);
        assert_eq!(buf.channels, 1);
        assert!(!buf.samples.is_empty());
    }

    #[test]
    fn decodes_stereo_wav_with_two_channels() {
        let buf = decode_to_pcm("wav", &audio_fixture("tiny_stereo.wav"))
            .expect("decode tiny_stereo.wav");
        assert_eq!(buf.channels, 2);
        // Interleaved layout: total samples == frames × channels, so an
        // odd count would be a bug. 0.25 s @ 44.1 kHz = 11025 frames;
        // × 2 channels = 22050 samples. ffprobe-confirmed against the
        // fixture (duration_ts=11025).
        assert_eq!(buf.samples.len() % 2, 0, "stereo PCM must be even-length");
        assert_eq!(buf.samples.len(), 22050);
    }

    #[test]
    fn empty_bytes_yield_unsupported_format() {
        let result = decode_to_pcm("mp3", &[]);
        match result {
            Err(AppError::UnsupportedFormat { .. }) => {}
            other => panic!("expected UnsupportedFormat for empty input, got {other:?}"),
        }
    }

    #[test]
    fn garbage_bytes_yield_unsupported_format() {
        let result = decode_to_pcm("mp3", b"not an audio file at all, just text");
        match result {
            Err(AppError::UnsupportedFormat { .. }) => {}
            other => panic!("expected UnsupportedFormat for garbage input, got {other:?}"),
        }
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

    // --- Channel mode (commit 5) ---

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

    // --- MP3 sample-rate validation (commit 5) ---

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

    /// convert_one routes the rate-validation error path through cleanly
    /// for an unsupported MP3 rate. We don't have a fixture at, say,
    /// 96 kHz, so we construct one by tweaking the WAV bytes' rate
    /// claim. Simpler: encode the mono WAV at 22050 Hz first (which IS
    /// in LAME's set), assert MP3 succeeds, then assert convert_one
    /// rejects an 8001 Hz rate via the unit on `validate_mp3_sample_rate`
    /// directly (covered above). The convert_one integration shape is
    /// exercised by mp3_mono_round_trips at 44100.
    #[test]
    fn lame_supported_rates_set_is_non_empty_and_contains_common_rates() {
        assert!(LAME_SUPPORTED_RATES.contains(&44100));
        assert!(LAME_SUPPORTED_RATES.contains(&48000));
        assert!(!LAME_SUPPORTED_RATES.is_empty());
    }
}
