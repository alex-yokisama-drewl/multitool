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

use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use hound::{SampleFormat as HoundSampleFormat, WavSpec, WavWriter};
use serde::{Deserialize, Serialize};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

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

/// Convert a single audio file's bytes from its source format to
/// `opts.target_format`.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot)
/// and is consulted only when Symphonia's bytes-sniffer is inconclusive (rare
/// — most containers have a magic byte sequence). Bytes always win when a
/// format is identifiable from the stream.
///
/// WAV + FLAC are wired in this commit; MP3 + OGG follow in commit 4.
/// Channel + sample-rate compatibility (downmix, encoder-rate validation)
/// lands in commit 5.
pub fn convert_one(source_ext: &str, input_bytes: &[u8], opts: &Opts) -> AppResult<EncodedFile> {
    let decoded = decode_to_pcm(source_ext, input_bytes)?;
    let bytes = match opts.target_format {
        TargetFormat::Wav => encode_wav(&decoded, opts.wav_bit_depth)?,
        TargetFormat::Flac => encode_flac(&decoded, opts.flac_compression_level)?,
        TargetFormat::Mp3 | TargetFormat::Ogg => {
            return Err(AppError::ProcessingFailed {
                detail: format!(
                    "audio_format_converter::convert_one: encoder for {:?} not yet implemented",
                    opts.target_format
                ),
            });
        }
    };
    Ok(EncodedFile {
        bytes,
        warnings: Vec::new(),
    })
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

    // --- Stubs that should still fail until commit 4 lands MP3/OGG ---

    #[test]
    fn mp3_encoder_not_wired_yet() {
        let opts = default_opts(TargetFormat::Mp3);
        let result = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts);
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(
                    detail.contains("not yet implemented"),
                    "expected MP3-not-implemented sentinel, got {detail}"
                );
            }
            other => panic!("expected ProcessingFailed for MP3 stub, got {other:?}"),
        }
    }

    #[test]
    fn ogg_encoder_not_wired_yet() {
        let opts = default_opts(TargetFormat::Ogg);
        let result = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts);
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(
                    detail.contains("not yet implemented"),
                    "expected OGG-not-implemented sentinel, got {detail}"
                );
            }
            other => panic!("expected ProcessingFailed for OGG stub, got {other:?}"),
        }
    }
}
