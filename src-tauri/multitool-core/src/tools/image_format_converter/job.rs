//! Batch orchestrator for the Image Format Converter.
//!
//! [`run_job`] walks the picked input files, calls [`super::convert::convert_one`]
//! per file, writes successful outputs to disk via [`crate::fs::unique_path`],
//! and streams progress through the `on_progress` callback. Per-file failures
//! land in the skipped list and **do not abort** the job — that's the
//! "skip + continue" rule from the brief.
//!
//! Cancellation does abort the job (with `AppError::Cancelled`), and so do
//! orchestrator-level failures the caller can't recover from (e.g. invalid
//! input slice). Per-file `FileNotFound` / decode failures don't abort.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::Opts;
use crate::error::{AppError, AppResult};

/// Per-file event streamed to the UI as the job progresses.
///
/// `index` is 0-based in input-list order; `total` is the picked file count
/// and is constant across the job. Each file fires `Started` first, then
/// either `Succeeded` or `Skipped`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to read + convert this file.
    Started {
        index: u32,
        total: u32,
        source: PathBuf,
    },
    /// File converted and written to disk at `output`. `warnings` collects
    /// non-fatal per-file notes (animated GIF first-frame, SVG text, …).
    Succeeded {
        index: u32,
        total: u32,
        source: PathBuf,
        output: PathBuf,
        warnings: Vec<String>,
    },
    /// File skipped. The job continues with the next file.
    Skipped {
        index: u32,
        total: u32,
        source: PathBuf,
        error: AppError,
    },
}

/// One entry in the final summary's `skipped` list. The serialized form
/// matches the [`Progress::Skipped`] payload's relevant fields so the UI can
/// render either source equivalently.
#[derive(Clone, Debug, Serialize)]
pub struct SkippedFile {
    pub source: PathBuf,
    pub error: AppError,
}

/// Returned from [`run_job`] on a (non-cancelled, non-orchestrator-error) run.
/// Always represents a completed batch — even when every file was skipped,
/// the job result is `Ok(JobResult)` with `success_count = 0`.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    pub success_count: u32,
    pub skip_count: u32,
    pub skipped: Vec<SkippedFile>,
    /// Directory holding the first successful output (handy for "reveal in
    /// folder" UX). `None` when no file succeeded.
    pub first_output_dir: Option<PathBuf>,
    pub duration_ms: u64,
}

/// Reasons the orchestrator itself can fail (vs per-file skips):
/// - empty `inputs` slice → `AppError::ProcessingFailed`
/// - cancellation → `AppError::Cancelled`
///
/// Per-file decode failures, missing files, alpha-handling refusals, etc. land
/// in [`JobResult::skipped`] instead.
pub fn run_job<F>(
    _inputs: &[PathBuf],
    _opts: &Opts,
    _on_progress: F,
    _cancel: &CancellationToken,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    // Scaffold stub. A5 lands the batch loop + unique_path + atomic writes.
    let _zero_duration = Duration::ZERO;
    Err(AppError::ProcessingFailed {
        detail: "image-format-converter run_job is a Phase-A1 scaffold stub".into(),
    })
}
