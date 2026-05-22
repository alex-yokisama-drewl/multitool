//! Tauri command for the Image Format Converter tool.
//!
//! Thin adapter over [`multitool_core::tools::image_format_converter::run_job`]:
//! moves the synchronous batch run onto a blocking thread, threads a
//! [`CancellationToken`](tokio_util::sync::CancellationToken) through the job
//! registry, and translates streaming `on_progress` events into
//! `tool:progress` events on the Tauri event bus.
//!
//! Mirrors `src/tools/images_to_pdf/mod.rs` deliberately — Phase F will
//! decide whether the three shims collapse into a shared shell helper.
//! Per `CLAUDE.md` → "Workspace split" the testable logic lives in
//! `multitool-core`; this shim's correctness is bounded by compile-time
//! tauri::command macro checking + the TS wrapper tests in B2 + the
//! Playwright lane in D1.

use std::path::PathBuf;

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::image_format_converter::{run_job, JobResult, Opts, Progress};
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
pub async fn convert_image_format<R: Runtime>(
    app: AppHandle<R>,
    job_id: JobId,
    paths: Vec<PathBuf>,
    opts: Opts,
    registry: State<'_, JobRegistry>,
) -> Result<JobResult, AppError> {
    let cancel = registry.register(job_id.clone());

    // run_job is synchronous: per-file read + decode + encode + write. Off
    // the main async runtime onto a blocking thread so the event loop stays
    // responsive (and so `cancel_job` can interleave with the work).
    let result = {
        let job_id = job_id.clone();
        let app = app.clone();
        tokio::task::spawn_blocking(move || {
            run_job(
                &paths,
                &opts,
                |progress| {
                    // Best-effort emit: a dropped event must not fail the job.
                    let _ = app.emit(
                        "tool:progress",
                        ProgressEvent {
                            job_id: job_id.clone(),
                            progress,
                        },
                    );
                    Ok(())
                },
                &cancel,
            )
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
