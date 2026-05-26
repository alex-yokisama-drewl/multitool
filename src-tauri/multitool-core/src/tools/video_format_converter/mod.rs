//! Video Format Converter — pure-logic side.
//!
//! Same shape as the other tools: types + per-file `convert` in
//! [`convert`], batch `run_job` + progress types in [`job`]. The Tauri
//! shell module in `src-tauri/src/tools/video_format_converter/` is a
//! thin shim over `run_job`. ffmpeg spawning + progress drainage lives in
//! [`crate::ffmpeg`] so any future video tool (Compress, Trim, Extract
//! Audio per `docs/plans/BACKLOG.md`) can reuse it.

mod convert;
mod job;

pub use convert::{convert, Opts, TargetFormat};
pub use job::{run_job, JobResult, Progress, SkippedFile};
