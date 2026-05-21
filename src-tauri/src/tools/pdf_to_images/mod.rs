//! Tauri command for the PDF → Images tool.
//!
//! Thin adapter over [`multitool_core::tools::pdf_to_images::run_job`]: it
//! moves the synchronous run onto a blocking thread, threads a
//! [`CancellationToken`](tokio_util::sync::CancellationToken) through the job
//! registry, and translates the streaming `on_progress` callback into
//! `tool:progress` events on the Tauri event bus.
//!
//! Per `CLAUDE.md` → "Workspace split", all behaviour worth testing lives in
//! `multitool-core` so it runs under `cargo test -p multitool-core` on every
//! CI OS. This module's correctness is bounded by compile-time `tauri::command`
//! macro checking + the JS-side wrapper tests in C7 + the Playwright
//! happy-path in C9. See `docs/tools/pdf-to-images-plan.md` → C6 for the
//! rationale behind dropping the planned `tauri::test` lane.

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::pdf_to_images::{run_job, JobResult, Opts, Progress};
use multitool_core::AppError;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

#[derive(Clone, Debug, Serialize)]
struct ProgressEvent {
    job_id: JobId,
    progress: Progress,
}

#[derive(Clone, Debug, Serialize)]
struct CompleteEvent {
    job_id: JobId,
    result: JobResult,
}

#[derive(Clone, Debug, Serialize)]
struct ErrorEvent {
    job_id: JobId,
    error: AppError,
}

#[tauri::command]
pub async fn convert_pdf_to_images<R: Runtime>(
    app: AppHandle<R>,
    job_id: JobId,
    path: PathBuf,
    opts: Opts,
    registry: State<'_, JobRegistry>,
) -> Result<JobResult, AppError> {
    let cancel = registry.register(job_id.clone());

    // `run_job` is synchronous: file I/O + pdfium rendering + image encode.
    // Off the main async runtime onto a blocking thread so the event loop
    // stays responsive (and so `cancel_job` can interleave with the render).
    let result = {
        let job_id = job_id.clone();
        let app = app.clone();
        tokio::task::spawn_blocking(move || {
            run_job(&path, &opts, &cancel, |progress| {
                // Best-effort emit: a dropped event must not fail the job.
                let _ = app.emit(
                    "tool:progress",
                    ProgressEvent {
                        job_id: job_id.clone(),
                        progress,
                    },
                );
                Ok(())
            })
        })
        .await
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("blocking task join failed: {err}"),
        })?
    };

    // Either the task finished naturally (we still own the token slot) or it
    // was cancelled via `cancel_job` (which already removed the slot). The
    // remove is idempotent.
    registry.unregister(&job_id);

    match &result {
        Ok(job_result) => {
            let _ = app.emit(
                "tool:complete",
                CompleteEvent {
                    job_id: job_id.clone(),
                    result: job_result.clone(),
                },
            );
        }
        Err(err) => {
            let _ = app.emit(
                "tool:error",
                ErrorEvent {
                    job_id: job_id.clone(),
                    error: err.clone(),
                },
            );
        }
    }

    result
}
