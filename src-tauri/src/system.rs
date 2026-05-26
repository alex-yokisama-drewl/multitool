//! System-level query commands that aren't tied to a single tool.
//!
//! Currently just exposes the pipeline's encodable-raster-format set so the
//! frontend has a single source of truth (the Rust [`RasterFormat`] enum)
//! for picker filters and format dropdowns instead of a hand-synced list.

use multitool_core::image::{RasterFormat, RasterFormatDescriptor};

/// Return every raster format the pipeline can both decode and encode.
///
/// Pure + cheap (a `const`-derived map over five variants); the frontend
/// memoizes the result so this is invoked at most once per app session.
#[tauri::command]
pub fn supported_raster_formats() -> Vec<RasterFormatDescriptor> {
    RasterFormat::all().iter().map(|f| f.descriptor()).collect()
}
