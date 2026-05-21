//! Pure conversion fn + types for the PDF → Images tool.
//!
//! The public surface is intentionally small: types in this module, the
//! `convert` entry point in [`convert`]. The Tauri shell adds command
//! plumbing on top in `src-tauri/src/tools/pdf_to_images/`.
//!
//! Per-tool spec: `docs/tools/pdf-to-images.md`. Build plan: `docs/tools/pdf-to-images-plan.md`.

mod convert;

pub use convert::{convert, Format, JobSummary, Opts, PageOutput};
