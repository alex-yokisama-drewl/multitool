//! Audio Trimmer — pure-logic side.
//!
//! Same shape as the other tools: types + pure transform in `convert.rs`,
//! single-file `run_job` + progress types in `job.rs`. The Tauri shell
//! module in `src-tauri/src/tools/audio_trimmer/` is a thin shim.
//!
//! Encode + decode primitives are reused from [`crate::audio_codecs`] —
//! the trimmer never duplicates Symphonia/claxon/hound/flacenc/LAME/vorbis_rs.
//!
//! v1 scope: one file at a time (the picker is single-select), source
//! format preserved (no transcode option), milliseconds-precision
//! `[start, end]` range with optional linear fade-in / fade-out. The
//! UI exposes fades as checkboxes that toggle a 1000 ms default; the
//! Rust `Opts` keeps ms integers so unit tests can hit edge cases
//! (zero, equal-to-window, overlap) directly.

mod convert;
mod job;

pub use convert::{trim_and_fade, Opts};
pub use job::{run_job, JobResult, Progress};
