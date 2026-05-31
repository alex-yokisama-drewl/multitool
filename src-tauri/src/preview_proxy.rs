//! Preview-proxy Tauri commands, shared by every video tool whose pick
//! flow needs a web-friendly preview.
//!
//! Three commands, hoisted out of `tools/video_trimmer/mod.rs` so the
//! Video Compressor (and any future video tool) can drive the same
//! transcode + cleanup pipeline without duplicating shim code:
//! - [`prepare_preview_proxy`] — transcodes a WebM proxy into the OS
//!   temp dir and grants asset-protocol scope on it before returning.
//! - [`cleanup_preview_proxy`] — best-effort delete of one proxy when a
//!   tool's session ends.
//! - [`cleanup_stale_proxies`] — sweeps every proxy left in the temp
//!   dir. Called on tool-mount so a hard app-close (which tears down
//!   the WebView before React unmounts) doesn't leak proxies forever.
//!
//! Path guards: every `cleanup_*` only touches files that sit directly
//! in the OS temp dir AND start with [`PROXY_PREFIX`], so a stray IPC
//! call can't turn `cleanup_preview_proxy` into an arbitrary-file
//! unlink.

use std::path::{Path, PathBuf};

use multitool_core::ipc::{JobId, JobRegistry};
use multitool_core::video::generate_proxy;
use multitool_core::AppError;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

/// Filename prefix for preview proxies in the OS temp dir. Also the
/// guard [`cleanup_preview_proxy`] / [`cleanup_stale_proxies`] check
/// before unlinking.
pub const PROXY_PREFIX: &str = "multitool-preview-";

/// Mid-transcode progress for the preview proxy. `fraction` is in
/// `[0.0, 1.0]`. Streamed on `tool:progress` like every other job.
#[derive(Clone, Debug, Serialize)]
pub struct ProxyProgress {
    pub fraction: f64,
}

/// Path of the generated preview proxy (a WebM in the OS temp dir).
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
    // One proxy per job id keeps temp names unique + correlatable. WebM so
    // WebKitGTK can decode it without proprietary codecs (see
    // multitool_core::video::proxy).
    let dest = std::env::temp_dir().join(format!("{PROXY_PREFIX}{}.webm", job_id.0));
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

/// Sweep every preview proxy left in the temp dir. Called when a tool
/// mounts so proxies orphaned by a hard app-close (Tauri tears the
/// WebView down without running React cleanup, so the in-flight proxy
/// survives) get reclaimed on the next launch. They're throwaway
/// previews, so deleting all of them is safe.
#[tauri::command]
pub fn cleanup_stale_proxies() -> Result<(), AppError> {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_owned_proxy(&path) {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

/// A path is a deletable proxy only if it sits directly in the OS temp
/// dir and its filename carries our [`PROXY_PREFIX`]. Keeps a stray IPC
/// call from turning `cleanup_preview_proxy` into an arbitrary-file
/// unlink.
fn is_owned_proxy(path: &Path) -> bool {
    let in_temp = path.parent() == Some(std::env::temp_dir().as_path());
    let named = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(PROXY_PREFIX) && n.ends_with(".webm"));
    in_temp && named
}
