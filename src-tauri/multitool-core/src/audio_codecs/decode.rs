//! Decode any supported audio file's bytes into interleaved f32 PCM.
//!
//! Decode path: [`symphonia`] (pure Rust, decode-only). FLAC routes
//! separately through [`claxon`] because Symphonia 0.6's FLAC demuxer
//! is strict about STREAMINFO's `total_samples` matching the demuxed
//! frame count, and our own `flacenc`-produced output trips that check.
//! See DECISIONS.md → "Audio stack" for the rationale.

use std::io::Cursor;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::audio_codecs::AudioBuffer;
use crate::error::{AppError, AppResult};

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
/// the caller turns into a skipped-file event for batch tools.
pub fn decode_to_pcm(source_ext: &str, bytes: &[u8]) -> AppResult<AudioBuffer> {
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
}
