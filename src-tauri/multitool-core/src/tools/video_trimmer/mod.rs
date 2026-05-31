//! Video Trimmer — pure-logic side.
//!
//! Same shape as the other tools: types + single-file `convert` in
//! [`convert`], single-file `run_job` + progress types in [`job`], plus a
//! [`probe`] helper the UI calls to learn the source duration before
//! trimming. The preview-proxy transcode (used by the pick flow) lives in
//! [`crate::video::proxy`] now that it's shared with other video tools;
//! re-exported here so the shell shim path stays stable. The Tauri shell
//! module in `src-tauri/src/tools/video_trimmer/` is a thin shim. ffmpeg
//! spawning + progress drainage is reused from [`crate::ffmpeg`].

mod convert;
mod job;
mod probe;

pub use crate::video::proxy::generate_proxy;
pub use convert::{convert, Opts};
pub use job::{run_job, JobResult, Progress};
pub use probe::probe_duration_ms;
