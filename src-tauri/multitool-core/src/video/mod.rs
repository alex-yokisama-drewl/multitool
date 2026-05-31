//! Shared video primitives consumed by every tool in `tools/<video_*>/`.
//!
//! `proxy` owns the WebM/VP9/Opus preview-proxy transcode (the trimmer + the
//! compressor both call it during pick-flow). Codec-mirror tables for
//! re-encoding will land alongside in a follow-up commit.

pub mod proxy;

pub use proxy::generate_proxy;
