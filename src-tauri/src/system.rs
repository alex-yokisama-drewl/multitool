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
