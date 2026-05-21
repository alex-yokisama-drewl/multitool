//! Tauri command wrappers around `multitool_core::ipc`.
//!
//! The pure types live in `multitool-core` so they can be unit-tested
//! without spinning up Tauri. This module is the thin glue that exposes
//! cancellation to the webview.

pub use multitool_core::ipc::{JobId, JobRegistry};

#[tauri::command]
pub fn cancel_job(job_id: JobId, registry: tauri::State<'_, JobRegistry>) -> bool {
    registry.cancel(&job_id)
}
