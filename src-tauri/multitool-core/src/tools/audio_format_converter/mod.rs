//! Audio Format Converter — pure-logic side.
//!
//! Same shape as the other tools: types + `convert_one` in `convert.rs`,
//! batch `run_job` + progress types in `job.rs`. The Tauri shell module in
//! `src-tauri/src/tools/audio_format_converter/` is a thin shim over
//! `run_job`.

mod convert;
mod job;

pub use convert::{convert_one, ChannelMode, EncodedFile, Opts, TargetFormat, WavBitDepth};
pub use job::{run_job, JobResult, Progress, SkippedFile};
