//! Tauri commands for the Video Trimmer tool.
//!
//! Four commands:
//! - [`trim_video`] — the stream-copy trim, a thin shim over
//!   [`multitool_core::tools::video_trimmer::run_job`] via
//!   [`crate::ipc::run_streaming_job`] (same shape as `trim_audio`).
//! - [`probe_video_duration`] — synchronous duration probe so the picked
//!   view can size the trim window before running.
//! - [`prepare_preview_proxy`] — transcodes a web-friendly preview proxy
//!   for sources the WebView can't decode natively, granting asset-protocol
//!   scope on the proxy so the player can load it.
//! - [`cleanup_preview_proxy`] — best-effort delete of a proxy once the
//!   user moves on. Guarded to the OS temp dir + our filename prefix so a
//!   stray IPC call can't unlink arbitrary files.

use std::path::{Path, PathBuf};

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::tools::video_trimmer::{
    generate_proxy, probe_duration_ms, run_job, JobResult, Opts,
};
use multitool_core::AppError;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

/// Filename prefix for preview proxies in the OS temp dir. Also the guard
/// [`cleanup_preview_proxy`] checks before unlinking.
const PROXY_PREFIX: &str = "multitool-preview-";

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

/// Mid-transcode progress for the preview proxy. `fraction` is in
/// `[0.0, 1.0]`. Streamed on `tool:progress` like every other job.
#[derive(Clone, Debug, Serialize)]
pub struct ProxyProgress {
    pub fraction: f64,
}

/// Path of the generated preview proxy (an mp4 in the OS temp dir).
#[derive(Clone, Debug, Serialize)]
pub struct ProxyResult {
    pub proxy_path: PathBuf,
}

#[tauri::command]
pub async fn prepare_preview_proxy<R: Runtime>(
    app: AppHandle<R>,
    job_id: JobId,
    path: PathBuf,
    registry: State<'_, JobRegistry>,
) -> Result<ProxyResult, AppError> {
    // One proxy per job id keeps temp names unique + correlatable.
    let dest = std::env::temp_dir().join(format!("{PROXY_PREFIX}{}.mp4", job_id.0));
    let scope_app = app.clone();

    crate::ipc::run_streaming_job(app, registry, job_id, move |cancel, emit| {
        generate_proxy(
            &path,
            &dest,
            |fraction| {
                let _ = emit(ProxyProgress { fraction });
            },
            &cancel,
        )?;
        // Grant asset-protocol scope on the proxy *before* returning, so by
        // the time `tool:complete` fires the webview can already resolve
        // `convertFileSrc(proxy_path)` without a race.
        scope_app
            .asset_protocol_scope()
            .allow_file(&dest)
            .map_err(|err| AppError::ProcessingFailed {
                detail: format!("failed to extend asset protocol scope for proxy: {err}"),
            })?;
        Ok(ProxyResult { proxy_path: dest })
    })
    .await
}

#[tauri::command]
pub fn cleanup_preview_proxy(path: PathBuf) -> Result<(), AppError> {
    if !is_owned_proxy(&path) {
        return Err(AppError::ProcessingFailed {
            detail: format!(
                "refusing to delete {}: not a multitool preview proxy in the temp dir",
                path.display()
            ),
        });
    }
    // Best-effort: a missing file (already cleaned / never created) is fine.
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(AppError::ProcessingFailed {
            detail: format!("failed to delete preview proxy {}: {err}", path.display()),
        }),
    }
}

/// A path is a deletable proxy only if it sits directly in the OS temp dir
/// and its filename carries our [`PROXY_PREFIX`]. Keeps a stray IPC call
/// from turning `cleanup_preview_proxy` into an arbitrary-file unlink.
fn is_owned_proxy(path: &Path) -> bool {
    let in_temp = path.parent() == Some(std::env::temp_dir().as_path());
    let named = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(PROXY_PREFIX) && n.ends_with(".mp4"));
    in_temp && named
}
