//! Batch orchestrator for the Audio Format Converter.
//!
//! Mirrors `image_format_converter::job` in shape — same `Progress` /
//! `JobResult` wire types, same skip + continue rule, same cancellation
//! semantics. The only meaningful difference (when filled in across the
//! follow-up commits) is that cancellation is also checked **inside** the
//! per-file encode loop, every ~50 ms of audio, because a single audio file
//! can be tens of minutes long — leaving cancellation as a between-files
//! check only would feel broken in practice.

use std::path::PathBuf;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::Opts;
use crate::error::{AppError, AppResult};

/// Per-file event streamed to the UI as the job progresses.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    Started {
        index: u32,
        total: u32,
        source: PathBuf,
    },
    Succeeded {
        index: u32,
        total: u32,
        source: PathBuf,
        output: PathBuf,
        warnings: Vec<String>,
    },
    Skipped {
        index: u32,
        total: u32,
        source: PathBuf,
        error: AppError,
    },
}

/// One entry in the final summary's `skipped` list.
#[derive(Clone, Debug, Serialize)]
pub struct SkippedFile {
    pub source: PathBuf,
    pub error: AppError,
}

/// Result of a completed batch run (`run_job` returned `Ok`).
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    pub success_count: u32,
    pub skip_count: u32,
    pub skipped: Vec<SkippedFile>,
    pub first_output_path: Option<PathBuf>,
    pub duration_ms: u64,
}

/// Run the full batch end-to-end.
///
/// Stub for commit 1. Real orchestration lands in commit 6.
pub fn run_job<F>(
    _inputs: &[PathBuf],
    _opts: &Opts,
    _cancel: &CancellationToken,
    mut _on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    Err(AppError::ProcessingFailed {
        detail: "audio_format_converter::run_job not yet implemented".into(),
    })
}
