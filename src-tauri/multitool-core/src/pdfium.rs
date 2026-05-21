//! Loader for the dynamic pdfium binary.
//!
//! The native library is downloaded by `build.rs` from
//! <https://github.com/bblanchon/pdfium-binaries> at the pinned `chromium/7763`
//! release. Its on-disk path is baked into the binary at compile time via the
//! `PDFIUM_LIB_PATH` env var that `build.rs` sets.
//!
//! Phase 1 limitation: the baked path points into the build machine's
//! `target/build/.../out/pdfium/...` tree, which is fine for dev and tests but
//! will not exist on an end user's machine in a bundled release. C6 (the
//! Tauri command wiring) will own re-resolving this to a Tauri resource path
//! before we hand the binary to users. Captured in `DECISIONS.md`.

use pdfium_render::prelude::{Pdfium, PdfiumError, PdfiumLibraryBindings};

/// Bind to the pdfium library that `build.rs` staged for this target.
pub fn bindings() -> Result<Box<dyn PdfiumLibraryBindings>, PdfiumError> {
    Pdfium::bind_to_library(env!("PDFIUM_LIB_PATH"))
}
