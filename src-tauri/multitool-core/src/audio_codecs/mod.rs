//! Shared audio decode + encode primitives.
//!
//! Lifted out of [`crate::tools::audio_format_converter`] so a second audio
//! tool (the Audio Trimmer in `tools::audio_trimmer`) can reuse the same
//! decoder and encoder surface without duplicating Symphonia / claxon /
//! hound / flacenc / mp3lame-encoder / vorbis_rs plumbing. Layered as:
//!
//! - [`AudioBuffer`] — the interleaved-f32 intermediate every encoder
//!   consumes and every decoder produces. Channels are an exact count;
//!   sample rate is passthrough from the source.
//! - [`decode`] — [`decode::decode_to_pcm`] is the single decode entry
//!   point; FLAC routes through `claxon`, everything else through
//!   Symphonia. See the module docs for the routing rationale.
//! - [`encode`] — one fn per target format: WAV / FLAC / MP3 / OGG Vorbis.
//!   Plus [`encode::validate_mp3_sample_rate`] for the LAME-set gate that
//!   both the converter and the trimmer apply before MP3-bound encodes.
//!
//! Why split mod-level rather than keeping everything in `convert.rs`?
//! Because the trimmer needs decode and the matching encoder for the
//! source format, but does NOT need [`super::tools::audio_format_converter`]'s
//! channel-mode policy, `TargetFormat` enum, or `convert_one` orchestrator.
//! Pulling decode/encode up keeps the converter's tool-shaped surface
//! intact while exposing exactly the bits the trimmer needs.

pub mod decode;
pub mod encode;

/// Decoded audio payload: interleaved f32 samples + the layout needed to
/// re-encode them.
///
/// Channel count is exact (Symphonia / claxon are already normalised
/// before we see them); sample rate is whatever the source ships and is
/// passed through to the encoder unchanged. Per the
/// "passthrough sample rate" decision in DECISIONS.md.
///
/// Encoders consume this directly. The audio_format_converter holds
/// onto [`AudioBuffer`] across an `apply_channel_mode` pass; the audio
/// trimmer holds onto it across a `trim_and_fade` pass.
#[derive(Clone, Debug)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
}
