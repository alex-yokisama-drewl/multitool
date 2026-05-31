//! Shared video primitives consumed by every tool in `tools/<video_*>/`.
//!
//! `proxy` owns the WebM/VP9/Opus preview-proxy transcode (the trimmer + the
//! compressor both call it during pick-flow). `reencode` owns the codec-mirror
//! tables (encoder selection + CRF range per source codec) the trimmer +
//! compressor share when building an ffmpeg arg vec for a re-encode.

pub mod proxy;
pub mod reencode;

pub use proxy::generate_proxy;
pub use reencode::{audio_encoder_flags, video_crf_range, video_encoder_flags};
