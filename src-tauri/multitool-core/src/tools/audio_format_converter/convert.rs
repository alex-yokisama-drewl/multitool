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
    /// MP3 CBR bitrate, kbps. Clamped in commits 3–4 when MP3 encoder lands.
    pub mp3_bitrate_kbps: u32,
    /// Vorbis quality, Xiph scale −1.0…10.0. Clamped in commits 3–4.
    pub vorbis_quality: f32,
    /// FLAC compression level 0…8. Clamped in commits 3–4.
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
/// Internal intermediate; not part of the tool's IPC surface. Encoders in
/// commits 3–4 consume this directly.
#[derive(Clone, Debug)]
// Fields are read only by tests in this commit; commit 3's WAV/FLAC
// encoders are the first non-test consumers, at which point this allow
// gets removed.
#[allow(dead_code)]
pub(super) struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
}

/// Decode an audio file's bytes into interleaved f32 PCM via Symphonia.
///
/// `source_ext` is passed to Symphonia's [`Hint`] only as a tie-breaker —
/// the actual format is detected from bytes when possible. Pass `""` (or
/// the lowercased extension without leading dot) when the caller has no
/// extension to offer; bytes-sniffing still wins.
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

/// Convert a single audio file's bytes from its source format to
/// `opts.target_format`.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot)
/// and is consulted only when Symphonia's bytes-sniffer is inconclusive (rare
/// — most containers have a magic byte sequence). Bytes always win when a
/// format is identifiable from the stream.
///
/// Stub for commits 2–4: decode is real but every encoder still returns
/// `ProcessingFailed`. Commit 3 fills in WAV + FLAC; commit 4 fills in
/// MP3 + OGG.
pub fn convert_one(source_ext: &str, input_bytes: &[u8], opts: &Opts) -> AppResult<EncodedFile> {
    let _decoded = decode_to_pcm(source_ext, input_bytes)?;
    Err(AppError::ProcessingFailed {
        detail: format!(
            "audio_format_converter::convert_one: encoder for {:?} not yet implemented",
            opts.target_format
        ),
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

    /// Sanity: `convert_one` decodes successfully but still returns the
    /// "encoder not yet implemented" error because no encoder is wired in
    /// yet. Once commit 3 lands, this test gets replaced by per-target
    /// round-trip tests.
    #[test]
    fn convert_one_decodes_but_no_encoder_wired_yet() {
        let opts = Opts {
            target_format: TargetFormat::Wav,
            mp3_bitrate_kbps: 192,
            vorbis_quality: 5.0,
            flac_compression_level: 5,
            wav_bit_depth: WavBitDepth::Bit16,
            channels: ChannelMode::Source,
        };
        let result = convert_one("wav", &audio_fixture("tiny_mono.wav"), &opts);
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(
                    detail.contains("not yet implemented"),
                    "expected the not-yet-implemented sentinel, got {detail}"
                );
            }
            other => panic!("expected ProcessingFailed not-yet-implemented, got {other:?}"),
        }
    }
}
