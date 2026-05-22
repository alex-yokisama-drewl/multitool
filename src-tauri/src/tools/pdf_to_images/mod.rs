//! Tauri command for the PDF → Images tool.
//!
//! Thin adapter over [`multitool_core::tools::pdf_to_images::run_job`]:
//! delegates the spawn_blocking / progress-emit / complete-or-error
//! boilerplate to [`crate::ipc::run_streaming_job`].

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::pdf_to_images::{run_job, JobResult, Opts};
use multitool_core::AppError;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub async fn convert_pdf_to_images<R: Runtime>(
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
