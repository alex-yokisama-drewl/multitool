//! Audio Format Converter — pure-logic side.
//!
//! Same shape as the other tools: types + `convert_one` in `convert.rs`,
//! batch `run_job` + progress types in `job.rs`. The Tauri shell module in
//! `src-tauri/src/tools/audio_format_converter/` is a thin shim over
//! `run_job`. Encode + decode primitives live in
//! [`crate::audio_codecs`] so the Audio Trimmer can reuse them.

mod convert;
mod job;

pub use crate::audio_codecs::encode::WavBitDepth;
pub use convert::{convert_one, ChannelMode, EncodedFile, Opts, TargetFormat};
pub use job::{run_job, JobResult, Progress, SkippedFile};
