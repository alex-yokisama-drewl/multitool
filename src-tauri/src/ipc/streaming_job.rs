//! Shared boilerplate for "spawn_blocking a synchronous job + stream
//! progress events to the webview + emit complete/error on join".
//!
//! Every tool's `#[tauri::command]` shim does the same dance:
//! 1. Register the JobId on the [`JobRegistry`] → fetch a CancellationToken.
//! 2. `tokio::task::spawn_blocking` the synchronous run.
//! 3. Forward each streaming progress event onto `tool:progress`.
//! 4. Unregister the JobId after the task joins.
//! 5. Emit `tool:complete` or `tool:error` based on the result.
//!
//! [`run_streaming_job`] absorbs (1), (2), (4), and (5). Each shim provides
//! a `run` closure that calls its tool's `run_job` with the supplied
//! cancellation token and progress-emitter.
//!
//! The helper is intentionally crate-internal — `pub(crate)` only, since
//! the event names + envelope shapes are an IPC contract negotiated with
//! the webview wrappers in `src/lib/jobRunner.ts`.

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::{AppError, AppResult};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};
use tokio_util::sync::CancellationToken;

/// Wire shape of a `tool:progress` event. Generic over the per-tool
/// `Progress` payload so each shim doesn't define a local copy.
#[derive(Clone, Debug, Serialize)]
struct ProgressEvent<P> {
    job_id: JobId,
    progress: P,
}

/// Wire shape of a `tool:complete` event.
#[derive(Clone, Debug, Serialize)]
struct CompleteEvent<R> {
    job_id: JobId,
    result: R,
}

/// Wire shape of a `tool:error` event.
#[derive(Clone, Debug, Serialize)]
struct ErrorEvent {
    job_id: JobId,
    error: AppError,
}

/// Type alias for the emitter closure handed to each tool's `run` callback.
/// `Box<dyn Fn>` (not `FnMut`) because `tauri::AppHandle::emit` takes `&self`
/// — emitting is internally synchronized, so we don't need `&mut`. The
/// boxing is a single heap allocation per job, dwarfed by the actual work.
type ProgressEmitter<P> = Box<dyn Fn(P) -> AppResult<()> + Send + Sync + 'static>;

/// Run a streaming Tauri job end-to-end. See module-level docs.
///
/// `run` is the only per-tool piece: it receives the cancellation token + a
/// boxed progress emitter, calls the tool's `run_job` (in whatever arg
/// order it wants), and returns the tool-specific `Res`.
pub(crate) async fn run_streaming_job<TR, P, Res, Run>(
    app: AppHandle<TR>,
    registry: State<'_, JobRegistry>,
    job_id: JobId,
    run: Run,
) -> AppResult<Res>
where
    TR: Runtime,
    // `Clone` because Tauri's `emit` payload is `Clone + Serialize` (the
    // event router clones the payload across listeners).
    P: Clone + Serialize + Send + 'static,
    Res: Clone + Serialize + Send + 'static,
    Run: FnOnce(CancellationToken, ProgressEmitter<P>) -> AppResult<Res> + Send + 'static,
{
    let cancel = registry.register(job_id.clone());

    let emit: ProgressEmitter<P> = {
        let app = app.clone();
        let job_id = job_id.clone();
        Box::new(move |progress| {
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
    };

    let result = tokio::task::spawn_blocking(move || run(cancel, emit))
        .await
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("blocking task join failed: {err}"),
        })?;

    // Either the task finished naturally (we still own the token slot) or
    // it was cancelled via `cancel_job` (which already removed the slot).
    // unregister is idempotent.
    registry.unregister(&job_id);

    match &result {
        Ok(value) => {
            let _ = app.emit(
                "tool:complete",
                CompleteEvent {
                    job_id,
                    result: value.clone(),
                },
            );
        }
        Err(err) => {
            let _ = app.emit(
                "tool:error",
                ErrorEvent {
                    job_id,
                    error: err.clone(),
                },
            );
        }
    }

    result
}
