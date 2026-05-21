//! Process-wide singleton accessor for pdfium.
//!
//! pdfium's library bindings are global state — `Pdfium::bind_to_library`
//! checks an internal `OnceCell` and returns
//! `PdfiumLibraryBindingsAlreadyInitialized` on any second call. So
//! everything that needs pdfium goes through [`instance`], which initializes
//! once and hands out a shared `&'static Pdfium`.
//!
//! The native library was downloaded by `build.rs` from
//! <https://github.com/bblanchon/pdfium-binaries> at the pinned
//! `chromium/7763` release; its on-disk path is baked in at compile time via
//! the `PDFIUM_LIB_PATH` env var that `build.rs` sets.
//!
//! Phase 1 limitation: the baked path points into the build machine's
//! `target/build/.../out/pdfium/...` tree, which is fine for dev and tests
//! but will not exist on an end user's machine in a bundled release. C6 (the
//! Tauri command wiring) will own re-resolving this to a Tauri resource path
//! before we hand the binary to users. Captured in `DECISIONS.md`.

use std::sync::{Mutex, OnceLock};

use pdfium_render::prelude::Pdfium;

use crate::error::AppError;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

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

    let bindings = Pdfium::bind_to_library(env!("PDFIUM_LIB_PATH")).map_err(|err| {
        AppError::ProcessingFailed {
            detail: format!("pdfium bind failed: {err:?}"),
        }
    })?;
    let _ = PDFIUM.set(Pdfium::new(bindings));

    PDFIUM.get().ok_or_else(|| AppError::ProcessingFailed {
        detail: "pdfium initialization completed but OnceLock is empty".into(),
    })
}
