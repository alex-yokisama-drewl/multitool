//! Tauri command for the Image Crop tool.
//!
//! Thin adapter over [`multitool_core::tools::image_crop::run_job`]:
//! delegates the spawn_blocking / progress-emit / complete-or-error
//! boilerplate to [`crate::ipc::run_streaming_job`]. Same shape as the
//! Audio Trimmer shim — keep it boring.

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::image_crop::{run_job, CropRect, JobResult};
use multitool_core::AppError;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn crop_image<R: Runtime>(
    app: AppHandle<R>,
    job_id: JobId,
    path: PathBuf,
    opts: CropRect,
    registry: State<'_, JobRegistry>,
) -> Result<JobResult, AppError> {
    crate::ipc::run_streaming_job(app, registry, job_id, move |cancel, emit| {
        run_job(&path, &opts, &cancel, emit)
    })
    .await
}
