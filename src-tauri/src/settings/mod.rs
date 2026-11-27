//! Tauri commands for the durable user settings document.
//!
//! A thin shim: path resolution lives in [`crate::fs::settings_path`], and
//! every behaviour worth testing lives in [`multitool_core::settings`] (the
//! shell crate has no test lane — see ARCHITECTURE §4).
//!
//! The document is read once at app start and written back whole on a
//! frontend debounce. There is no partial-update command: the whole thing is
//! a few dozen short scalars, so a read-modify-write of the entire document
//! is cheaper than the coordination a merge would need.

use multitool_core::error::AppError;
use multitool_core::settings::{self, LoadOutcome, SettingsDoc};
use tauri::{AppHandle, Runtime};

/// Read the settings document, falling back to defaults.
///
/// Infallible by contract — `multitool_core::settings::load` resolves every
/// failure to defaults, so the frontend never has to handle a settings error
/// during boot. Recovery is logged, not surfaced.
#[tauri::command]
pub fn load_settings<R: Runtime>(app: AppHandle<R>) -> SettingsDoc {
    let path = match crate::fs::settings_path(&app) {
        Ok(path) => path,
        Err(err) => {
            tracing::warn!(
                target: "multitool::settings",
                error = %err,
                "could not resolve settings path; running with defaults"
            );
            return SettingsDoc::default();
        }
    };

    let (doc, outcome) = settings::load(&path);
    match outcome {
        LoadOutcome::Loaded => {
            tracing::debug!(
                target: "multitool::settings",
                path = %path.display(),
                entries = doc.entries.len(),
                "loaded settings"
            );
        }
        LoadOutcome::Missing => {
            tracing::debug!(
                target: "multitool::settings",
                path = %path.display(),
                "no settings file yet; using defaults"
            );
        }
        LoadOutcome::Recovered(reason) => {
            tracing::warn!(
                target: "multitool::settings",
                path = %path.display(),
                reason = %reason,
                "settings file unusable; using defaults (it will be overwritten on the next save)"
            );
        }
    }
    doc
}

/// Write the settings document.
///
/// Errors propagate so the frontend can tell a persistence failure (read-only
/// config dir, full disk) apart from silence. The UI treats it as
/// non-blocking: preferences stay live in memory for the session either way.
#[tauri::command]
pub fn save_settings<R: Runtime>(app: AppHandle<R>, doc: SettingsDoc) -> Result<(), AppError> {
    let path = crate::fs::settings_path(&app)?;
    settings::save(&path, &doc)
}
