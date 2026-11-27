//! System-level query commands that aren't tied to a single tool.

use std::path::PathBuf;

use multitool_core::error::AppError;
use multitool_core::image::{RasterFormat, RasterFormatDescriptor};

/// Return every raster format the pipeline can both decode and encode.
///
/// Pure + cheap (a `const`-derived map over five variants); the frontend
/// memoizes the result so this is invoked at most once per app session.
#[tauri::command]
pub fn supported_raster_formats() -> Vec<RasterFormatDescriptor> {
    RasterFormat::all().iter().map(|f| f.descriptor()).collect()
}

/// Stat a file and return its size in bytes.
///
/// Used by the Image Compress staging UI to show source sizes before the
/// run starts (the orchestrator surfaces sizes per-file after the run
/// completes, but the brief calls for size context in staging too).
/// Picking this over the `tauri-plugin-fs` dep keeps the plugin surface
/// thin — we don't need a generic FS API.
#[tauri::command]
pub fn file_size(path: PathBuf) -> Result<u64, AppError> {
    match std::fs::metadata(&path) {
        Ok(meta) => Ok(meta.len()),
        Err(err) => Err(match err.kind() {
            std::io::ErrorKind::NotFound => AppError::FileNotFound {
                path: path.display().to_string(),
            },
            std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
                path: path.display().to_string(),
            },
            _ => AppError::ProcessingFailed {
                detail: format!("stat {}: {err}", path.display()),
            },
        }),
    }
}

/// Report whether `path` exists and is a directory.
///
/// The frontend validates a remembered last-used directory before handing
/// it to the file dialog as `defaultPath`: removable drives get unplugged
/// and folders get deleted between sessions, and a dialog pointed at a
/// vanished path behaves differently on every platform. Returning `false`
/// lets the caller fall through to the next candidate.
///
/// Returns `bool` rather than `Result` on purpose — "I could not tell" and
/// "it is not there" lead to the same fallback, so a symlink loop or a
/// permission-denied stat is simply `false`. Same reasoning as
/// `file_size` picking a narrow command over a `tauri-plugin-fs` dep.
#[tauri::command]
pub fn directory_exists(path: PathBuf) -> bool {
    std::fs::metadata(&path).is_ok_and(|meta| meta.is_dir())
}
