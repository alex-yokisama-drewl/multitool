//! Image Format Converter — pure-logic side.
//!
//! Mirrors `pdf_to_images/` and `images_to_pdf/` in shape: types + the
//! per-file `convert_one` live in `convert.rs`; the batch `run_job`
//! orchestrator + progress types live in `job.rs`. The Tauri shell module in
//! `src-tauri/src/tools/image_format_converter/` (Phase B1) is a thin shim
//! over `run_job`.
//!
//! Per-tool spec: `docs/tools/image-format-converter.md`. Build plan:
//! `docs/tools/image-format-converter-plan.md`. Both ephemeral — deleted on
//! ship.

mod convert;
mod job;

pub use convert::{convert_one, AlphaHandling, EncodedFile, Opts, SvgRasterSize, TargetFormat};
pub use job::{run_job, JobResult, Progress, SkippedFile};
