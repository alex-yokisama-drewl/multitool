//! Video Trimmer — pure-logic side.
//!
//! Same shape as the other tools: types + single-file `convert` in
//! [`convert`], single-file `run_job` + progress types in [`job`], plus a
//! [`probe`] helper the UI calls to learn the source duration before
//! trimming. The Tauri shell module in `src-tauri/src/tools/video_trimmer/`
//! is a thin shim. ffmpeg spawning + progress drainage is reused from
//! [`crate::ffmpeg`].

mod convert;
mod job;
mod probe;

pub use convert::{convert, Opts};
pub use job::{run_job, JobResult, Progress};
pub use probe::probe_duration_ms;
