//! Runtime asset-protocol scope grants for picker-driven media previews.
//!
//! Tauri 2.x's asset protocol (`convertFileSrc()` in the webview) is gated by
//! the scope configured in `tauri.conf.json` under `app.security.assetProtocol`.
//! We keep that scope empty by default and grant access per-picked-file at
//! runtime via this command, so the webview can only resolve media the user
//! deliberately picked through a picker — not arbitrary paths on disk. See
//! `DECISIONS.md` → "Asset protocol scope: dynamic per-pick".
//!
//! The OS picker's extension filter is advisory (a determined caller can
//! invoke this command with any path). We re-validate the extension here so a
//! direct IPC call can't widen the grant past supported media files.
//!
//! Generalised from the image-only `allow_image_preview` shape so the Audio
//! Trimmer (and any future audio/video preview tool) can route through the
//! same command instead of duplicating the per-path scope-grant loop. New
//! media families add an extension list and a `*_EXTS` slice to the allowlist.

use std::path::{Path, PathBuf};

use multitool_core::AppError;
use tauri::{AppHandle, Manager, Runtime};

// Every extension `pickConvertibleImages` (image-format-converter) +
// `pickImageFiles` (images-to-pdf) might pass through. Widened beyond the
// browser's natively-renderable set on purpose: granting scope is decoupled
// from "will the webview render this". Formats the browser can't draw
// (tiff / qoi / tga / pnm family) still get a scope grant so the `<img>`
// URL resolves; the rendering just fails silently and the user sees the
// filename + fallback. No new attack surface — these are still files the
// user picked, and we reject anything outside this list.
const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif", "ico", "tga", "pbm", "pgm", "ppm",
    "pnm", "qoi", "svg",
];

// Every extension the Audio Trimmer picker (`pickAudioFile`) lets through.
// Restricted to the round-trip-capable set — the trimmer can't preserve
// source format for m4a/aac/aiff/caf/mkv/webm because we have no encoder
// for those. The browser's Web Audio API decodes all five via
// `AudioContext.decodeAudioData`, so the preview path works the same
// regardless of extension.
const AUDIO_EXTS: &[&str] = &["mp3", "wav", "flac", "ogg", "oga"];

fn is_media_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    IMAGE_EXTS
        .iter()
        .chain(AUDIO_EXTS.iter())
        .any(|allowed| allowed.eq_ignore_ascii_case(ext))
}

#[tauri::command]
pub fn allow_media_preview<R: Runtime>(
    app: AppHandle<R>,
    paths: Vec<PathBuf>,
) -> Result<(), AppError> {
    let scope = app.asset_protocol_scope();
    for path in &paths {
        if !is_media_path(path) {
            return Err(AppError::UnsupportedFormat {
                detail: format!(
                    "asset-scope grant refused: {} is not a supported media file",
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
