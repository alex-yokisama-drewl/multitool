//! Video Compress — pure-logic side.
//!
//! Single-slider quality knob mapped per source codec via
//! [`crate::video::reencode::video_crf_range`]. Output preserves the
//! source's container + video codec family; audio is stream-copied
//! bit-perfect in v1 (the "scale audio with the slider" follow-up is
//! deferred — see plans/BACKLOG.md). The Tauri shell module in
//! `src-tauri/src/tools/video_compress/` is a thin shim.

mod convert;
mod job;

pub use convert::{convert, Opts};
pub use job::{run_job, JobResult, Progress};
