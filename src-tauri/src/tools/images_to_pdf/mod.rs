//! Tauri command for the Images → PDF tool.
//!
//! Thin adapter over [`multitool_core::tools::images_to_pdf::run_job`]:
//! delegates the spawn_blocking / progress-emit / complete-or-error
//! boilerplate to [`crate::ipc::run_streaming_job`].

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::images_to_pdf::{run_job, JobResult, Opts};
use multitool_core::AppError;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn convert_images_to_pdf<R: Runtime>(
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
