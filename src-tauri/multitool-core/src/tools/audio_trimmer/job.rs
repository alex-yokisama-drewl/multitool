//! Audio Trimmer — single-file orchestrator (scaffold).
//!
//! Mirrors the other tools' [`Progress`] / [`JobResult`] shape on the
//! wire so the shared `runJob` IPC layer can drive it. Single file in,
//! single file out — there's no `Skipped` event (per-file failure
//! becomes a top-level `AppError::ProcessingFailed` because there's
//! nothing to skip *to*).
//!
//! The encode + write path lands in commit 3. For now the orchestrator
//! returns `ProcessingFailed { detail: "audio_trimmer: not implemented yet" }`
//! so the Tauri command shim + frontend wrapper can be exercised end-to-end
//! through the IPC pipeline.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::Opts;
use crate::error::{AppError, AppResult};

/// Per-file event streamed to the UI as the job progresses.
///
/// Single variant in v1: the trimmer fires `Started` once when decode
/// begins, then either resolves with [`JobResult`] (success) or rejects
/// with an `AppError` (failure). No skip-and-continue path — there's
/// only one file.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to decode + trim + encode the picked file.
    Started { source: PathBuf },
}

/// Result of a completed trim.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    /// The path the trimmed bytes were written to. Routes through
    /// `multitool_core::fs::unique_path` so a same-name collision lands
    /// at `{stem}_trimmed (1).{ext}`.
    pub output: PathBuf,
    /// Per-file notes (e.g. overlap-clamp). Empty on the happy path.
    pub warnings: Vec<String>,
    pub duration_ms: u64,
}

/// Run the trim job end-to-end. Returns once the trimmed bytes are
/// written, or earlier with an `AppError` if anything fails.
///
/// Cancellation is checked before decode and before encode; mid-encode
/// cancel inherits the same v1 limitation as the Audio Format Converter
/// (encoders accept the full PCM buffer at once).
#[allow(clippy::needless_pass_by_value)]
pub fn run_job<F>(
    _source: &Path,
    _opts: &Opts,
    _cancel: &CancellationToken,
    mut _on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    Err(AppError::ProcessingFailed {
        detail: "audio_trimmer: not implemented yet".into(),
    })
}
