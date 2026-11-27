//! Path resolution and duplicate-name handling.
//!
//! Helpers (e.g. `unique_path`, output-folder derivation) land here as the
//! first tool needs them. See ARCHITECTURE §3.3 for the naming and
//! duplicate-handling rules these helpers must enforce.

use std::path::PathBuf;

use multitool_core::error::AppError;
use tauri::Manager;

/// Filename of the user settings document inside the app-config directory.
const SETTINGS_FILENAME: &str = "settings.json";

/// Absolute path to the user settings document.
///
/// Lives in Tauri's per-app config directory (`~/.config/<id>` on Linux,
/// `~/Library/Application Support/<id>` on macOS, `%APPDATA%\<id>` on
/// Windows) — the OS-blessed home for a file the user may legitimately want
/// to inspect or delete. The directory is not created here; `settings::save`
/// creates it on first write.
///
/// This is the first helper that earns this module: resolving it genuinely
/// needs the Tauri path API, which is why it can't live in
/// `multitool_core::fs` with the pure path helpers.
pub fn settings_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, AppError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(SETTINGS_FILENAME))
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("resolve app config dir: {err}"),
        })
}
