#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod asset_scope;
pub mod fs;
pub mod ipc;
pub mod tools;

use tauri::Manager;

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // `try_init` is a no-op if a subscriber is already installed (tests, etc.).
    let _ = fmt().with_env_filter(filter).try_init();
}

// Filename the pdfium native binary is staged under in `bundle.resources` by
// `build.rs`. Must match the `dest_name` arm picked in `stage_pdfium_resource`.
#[cfg(target_os = "windows")]
const PDFIUM_RESOURCE_FILENAME: &str = "pdfium.dll";
#[cfg(target_os = "macos")]
const PDFIUM_RESOURCE_FILENAME: &str = "libpdfium.dylib";
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
const PDFIUM_RESOURCE_FILENAME: &str = "libpdfium.so";

// Filename the ffmpeg sidecar binary is staged under in `bundle.resources` by
// `build.rs`. Must match the `dest_name` arm picked in `stage_ffmpeg_resource`.
#[cfg(target_os = "windows")]
const FFMPEG_RESOURCE_FILENAME: &str = "ffmpeg.exe";
#[cfg(not(target_os = "windows"))]
const FFMPEG_RESOURCE_FILENAME: &str = "ffmpeg";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(ipc::JobRegistry::default())
        .setup(|app| {
            // Point multitool-core at the bundled pdfium binary before any
            // PDF tool runs. In dev (`pnpm tauri dev`) the resource may not
            // be present; if it isn't, `pdfium::instance` falls back to the
            // compile-time `PDFIUM_LIB_PATH` baked by
            // `multitool-core/build.rs`, which points at the developer's
            // `OUT_DIR` copy. Bundled installs (the v0.2.0 bug we are
            // fixing) only have the resource path.
            let resource_subpath = format!("resources/pdfium/{PDFIUM_RESOURCE_FILENAME}");
            match app
                .path()
                .resolve(&resource_subpath, tauri::path::BaseDirectory::Resource)
            {
                Ok(path) if path.exists() => {
                    multitool_core::pdfium::init(path);
                }
                Ok(path) => {
                    tracing::debug!(
                        target: "multitool::pdfium",
                        path = %path.display(),
                        "bundled pdfium resource not present; falling back to build-time path"
                    );
                }
                Err(err) => {
                    tracing::debug!(
                        target: "multitool::pdfium",
                        error = %err,
                        "failed to resolve bundled pdfium resource path; falling back to build-time path"
                    );
                }
            }

            // Same fall-back contract for the bundled ffmpeg sidecar — used
            // by the Video Format Converter family of tools.
            let ffmpeg_subpath = format!("resources/ffmpeg/{FFMPEG_RESOURCE_FILENAME}");
            match app
                .path()
                .resolve(&ffmpeg_subpath, tauri::path::BaseDirectory::Resource)
            {
                Ok(path) if path.exists() => {
                    multitool_core::ffmpeg::init(path);
                }
                Ok(path) => {
                    tracing::debug!(
                        target: "multitool::ffmpeg",
                        path = %path.display(),
                        "bundled ffmpeg resource not present; falling back to build-time path"
                    );
                }
                Err(err) => {
                    tracing::debug!(
                        target: "multitool::ffmpeg",
                        error = %err,
                        "failed to resolve bundled ffmpeg resource path; falling back to build-time path"
                    );
                }
            }
            Ok(())
        });
    let builder = tools::register_commands(builder);

    if let Err(err) = builder.run(tauri::generate_context!()) {
        eprintln!("fatal: failed to run tauri application: {err}");
        std::process::exit(1);
    }
}
