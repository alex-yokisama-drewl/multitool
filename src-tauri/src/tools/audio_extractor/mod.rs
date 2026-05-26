//! Tauri command for the Audio Extractor tool.
//!
//! Thin adapter over [`multitool_core::tools::audio_extractor::run_job`]:
//! delegates the spawn_blocking / progress-emit / complete-or-error
//! boilerplate to [`crate::ipc::run_streaming_job`]. No per-tool `Opts`
//! to thread through (v1 has zero user-facing knobs); just the source
//! path.

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::audio_extractor::{run_job, JobResult};
use multitool_core::AppError;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn extract_audio<R: Runtime>(
    app: AppHandle<R>,
    job_id: JobId,
    path: PathBuf,
    registry: State<'_, JobRegistry>,
) -> Result<JobResult, AppError> {
    crate::ipc::run_streaming_job(app, registry, job_id, move |cancel, emit| {
        run_job(&path, &cancel, emit)
    })
    .await
}
