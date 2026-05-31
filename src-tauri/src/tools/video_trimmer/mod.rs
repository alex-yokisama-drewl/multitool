//! Tauri commands for the Video Trimmer tool.
//!
//! Two commands:
//! - [`trim_video`] — the frame-accurate codec-matched re-encode trim, a
//!   thin shim over [`multitool_core::tools::video_trimmer::run_job`]
//!   via [`crate::ipc::run_streaming_job`] (same shape as `trim_audio`).
//! - [`probe_video_duration`] — synchronous duration probe so the picked
//!   view can size the trim window before running.
//!
//! Preview-proxy generation + cleanup commands live in
//! [`crate::preview_proxy`] now — they're shared with every video tool
//! whose pick flow needs a WebM proxy preview.

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::video_trimmer::{probe_duration_ms, run_job, JobResult, Opts};
use multitool_core::AppError;
use serde::Serialize;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn trim_video<R: Runtime>(
    app: AppHandle<R>,
    job_id: JobId,
    path: PathBuf,
    opts: Opts,
    registry: State<'_, JobRegistry>,
) -> Result<JobResult, AppError> {
    crate::ipc::run_streaming_job(app, registry, job_id, move |cancel, emit| {
        run_job(&path, &opts, &cancel, emit)
    })
    .await
}

/// Probed source duration in milliseconds.
#[derive(Clone, Debug, Serialize)]
pub struct DurationResult {
    pub duration_ms: u64,
}

#[tauri::command]
pub async fn probe_video_duration(path: PathBuf) -> Result<DurationResult, AppError> {
    let duration_ms = tokio::task::spawn_blocking(move || probe_duration_ms(&path))
        .await
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("probe task join failed: {err}"),
        })??;
    Ok(DurationResult { duration_ms })
}
