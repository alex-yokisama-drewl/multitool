//! Image Compress — pure-logic side.
//!
//! Same shape as the other image tools: the per-file transform + types in
//! `convert.rs`, the batch orchestrator + progress/result types in `job.rs`.
//! The Tauri shell module in `src-tauri/src/tools/image_compress/` is a thin
//! shim over `run_job`.
//!
//! v1 scope: batch input (multi-select), per-source-format strategy
//! (JPEG stays JPEG, WebP stays WebP, lossless inputs route to lossy WebP),
//! single quality slider. Skip-and-continue on per-file failures; cancel
//! between files only.

mod convert;
mod job;

pub use convert::{compress_one, quality_to_q1to100, CompressedOutput, Opts};
pub use job::{run_job, CompressedFile, JobResult, Progress, SkippedFile};
