//! Audio Extractor — pure-logic side.
//!
//! One video input → N MP3 outputs, one per audio track in the source.
//! [`convert`] owns the per-track ffmpeg primitive (recipe + naming +
//! partial-file cleanup); [`job`] orchestrates: probes the track count up
//! front via [`crate::ffmpeg::probe_audio_stream_count`], then loops.
//!
//! Mirrors the structure of the Video Format Converter, with two notable
//! differences:
//! - Single input (not a batch). The iteration is over audio tracks
//!   within one file, not over a list of files.
//! - No `Skipped` event variant. Any per-track failure aborts the whole
//!   job; already-extracted prior tracks remain on disk.

mod convert;
mod job;

pub use job::{run_job, JobResult, Progress};
