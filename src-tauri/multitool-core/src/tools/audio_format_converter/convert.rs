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

use serde::{Deserialize, Serialize};

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
    /// MP3 CBR bitrate, kbps. Clamped to [`MP3_BITRATE_MIN`, `MP3_BITRATE_MAX`].
    pub mp3_bitrate_kbps: u32,
    /// Vorbis quality, Xiph scale −1.0…10.0. Clamped to
    /// [`VORBIS_QUALITY_MIN`, `VORBIS_QUALITY_MAX`].
    pub vorbis_quality: f32,
    /// FLAC compression level 0…8. Clamped to
    /// [`FLAC_LEVEL_MIN`, `FLAC_LEVEL_MAX`].
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

/// Convert a single audio file's bytes from its source format to
/// `opts.target_format`.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot)
/// and is consulted only when Symphonia's bytes-sniffer is inconclusive (rare
/// — most containers have a magic byte sequence). Bytes always win when a
/// format is identifiable from the stream.
///
/// Stub for commit 1. Real implementation lands across commits 2–5.
pub fn convert_one(_source_ext: &str, _input_bytes: &[u8], _opts: &Opts) -> AppResult<EncodedFile> {
    Err(AppError::ProcessingFailed {
        detail: "audio_format_converter::convert_one not yet implemented".into(),
    })
}
