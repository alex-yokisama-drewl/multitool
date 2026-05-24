//! Pure conversion fn + types for the Images → PDF tool.
//!
//! Mirrors `pdf_to_images/`'s shape: types and the `convert` entry point
//! live here; the Tauri shell command in `src-tauri/src/tools/images_to_pdf/`
//! wraps progress events onto `tool:progress` and writes the returned bytes
//! via the orchestrator in `job.rs`.

mod convert;
mod job;

pub use convert::{convert, JobSummary, Opts, PageProgress, PageSize};
pub use job::{run_job, JobResult, Progress};
