//! Runtime asset-protocol scope grants for picker-driven image previews.
//!
//! Tauri 2.x's asset protocol (`convertFileSrc()` in the webview) is gated by
//! the scope configured in `tauri.conf.json` under `app.security.assetProtocol`.
//! We keep that scope empty by default and grant access per-picked-file at
//! runtime via this command, so the webview can only resolve images the user
//! deliberately picked through `pickImageFiles()` — not arbitrary paths on
//! disk. See `DECISIONS.md` → "Asset protocol scope: dynamic per-pick".
//!
//! The OS picker's extension filter is advisory (a determined caller can
//! invoke this command with any path). We re-validate the extension here so a
//! direct IPC call can't widen the grant past image files.

use std::path::{Path, PathBuf};

use multitool_core::AppError;
use tauri::{AppHandle, Manager, Runtime};

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            IMAGE_EXTS
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false)
}

#[tauri::command]
pub fn allow_image_preview<R: Runtime>(
    app: AppHandle<R>,
    paths: Vec<PathBuf>,
) -> Result<(), AppError> {
    let scope = app.asset_protocol_scope();
    for path in &paths {
        if !is_image_path(path) {
            return Err(AppError::UnsupportedFormat {
                detail: format!(
                    "asset-scope grant refused: {} is not an image",
                    path.display()
                ),
            });
        }
        scope
            .allow_file(path)
            .map_err(|err| AppError::ProcessingFailed {
                detail: format!("failed to extend asset protocol scope: {err}"),
            })?;
    }
    Ok(())
}
