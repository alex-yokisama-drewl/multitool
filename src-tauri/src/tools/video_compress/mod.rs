//! Tauri command for the Video Compress tool.
//!
//! One command — [`compress_video`] — a thin shim over
//! [`multitool_core::tools::video_compress::run_job`] via
//! [`crate::ipc::run_streaming_job`] (same shape as `trim_video`).
//! Preview-proxy generation + cleanup live in [`crate::preview_proxy`],
//! shared with the trimmer.

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::video_compress::{run_job, JobResult, Opts};
use multitool_core::AppError;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn compress_video<R: Runtime>(
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
