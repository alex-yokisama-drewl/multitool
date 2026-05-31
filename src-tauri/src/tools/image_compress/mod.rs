//! Tauri command for the Image Compress tool.
//!
//! Thin adapter over [`multitool_core::tools::image_compress::run_job`]:
//! delegates the spawn_blocking / progress-emit / complete-or-error
//! boilerplate to [`crate::ipc::run_streaming_job`]. Same shape as the
//! Image Format Converter shim.

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::image_compress::{run_job, JobResult, Opts};
use multitool_core::AppError;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn compress_images<R: Runtime>(
    app: AppHandle<R>,
    job_id: JobId,
    paths: Vec<PathBuf>,
    opts: Opts,
    registry: State<'_, JobRegistry>,
) -> Result<JobResult, AppError> {
    crate::ipc::run_streaming_job(app, registry, job_id, move |cancel, emit| {
        run_job(&paths, &opts, &cancel, emit)
    })
    .await
}
