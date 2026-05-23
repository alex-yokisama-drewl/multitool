//! Process-wide singleton accessor for pdfium.
//!
//! pdfium's library bindings are global state — `Pdfium::bind_to_library`
//! checks an internal `OnceCell` and returns
//! `PdfiumLibraryBindingsAlreadyInitialized` on any second call. So
//! everything that needs pdfium goes through [`instance`], which initializes
//! once and hands out a shared `&'static Pdfium`.
//!
//! ## Path resolution
//!
//! Two sources are consulted, in order:
//!
//! 1. A path set at runtime via [`init`] — used by the Tauri shell to point
//!    at the `pdfium.dll` / `libpdfium.so` / `libpdfium.dylib` bundled as a
//!    Tauri resource alongside the installed binary.
//! 2. The compile-time `PDFIUM_LIB_PATH` env var baked by
//!    [`build.rs`](../../build.rs), which points into the multitool-core
//!    crate's `OUT_DIR`. Correct for dev (`cargo test`, `pnpm tauri dev`)
//!    where the file is on the developer's disk, but the path does not
//!    exist on an end user's machine.
//!
//! The runtime-init path is what fixes the "pdfium DLL not found on
//! installed Windows build" failure observed in v0.2.0 — see DECISIONS.md
//! → "pdfium: bundle native binary as a Tauri resource".

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use pdfium_render::prelude::Pdfium;

use crate::error::AppError;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
static OVERRIDE_PATH: OnceLock<PathBuf> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Set the path to the pdfium native library before the first [`instance`]
/// call. Subsequent calls are ignored — the path is locked in for the
/// lifetime of the process, mirroring pdfium's own one-shot binding.
///
/// The Tauri shell calls this from its setup hook with the bundled
/// resource path. If [`instance`] has already run (e.g. a dev build that
/// fell back to the compile-time env path), this is a no-op and the
/// already-loaded library remains in use.
pub fn init(path: PathBuf) {
    let _ = OVERRIDE_PATH.set(path);
}

/// Return the process-wide `Pdfium` instance, initializing it on first call.
///
/// Subsequent calls are a cheap atomic load. The first caller from each
/// process pays the cost of loading the native library; if that fails, the
/// error is surfaced as `AppError::ProcessingFailed`.
pub fn instance() -> Result<&'static Pdfium, AppError> {
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }

    // Serialize the slow path so racing callers don't both try to bind
    // (the second `bind_to_library` would fail with
    // `PdfiumLibraryBindingsAlreadyInitialized`). Poison recovery mirrors
    // `ipc::JobRegistry` — the mutex guards initialization, not data, so
    // a poisoned guard is still safe to use.
    let _guard = INIT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }

    let path: PathBuf = OVERRIDE_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(env!("PDFIUM_LIB_PATH")));

    let bindings = Pdfium::bind_to_library(&path).map_err(|err| AppError::ProcessingFailed {
        detail: format!("pdfium bind failed: {err:?}"),
    })?;
    let _ = PDFIUM.set(Pdfium::new(bindings));

    PDFIUM.get().ok_or_else(|| AppError::ProcessingFailed {
        detail: "pdfium initialization completed but OnceLock is empty".into(),
    })
}
