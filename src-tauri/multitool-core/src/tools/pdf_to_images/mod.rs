//! Pure conversion fn + types for the PDF → Images tool.
//!
//! The public surface is intentionally small: types in this module, the
//! `convert` entry point in [`convert`]. The Tauri shell adds command
//! plumbing on top in `src-tauri/src/tools/pdf_to_images/`.

mod convert;
mod job;
mod writer;

pub use convert::{convert, Format, JobSummary, Opts, PageOutput};
pub use job::{run_job, JobResult, Progress};
pub use writer::PageWriter;
