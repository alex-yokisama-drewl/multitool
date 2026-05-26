//! Image Crop — pure-logic side.
//!
//! Same shape as the other tools: the per-image transform + types live in
//! `convert.rs`; the single-file `run_job` orchestrator + progress/result
//! types live in `job.rs` (added in the next commit). The Tauri shell module
//! in `src-tauri/src/tools/image_crop/` is a thin shim over `run_job`.
//!
//! v1 scope: one image at a time (single-select picker), source format
//! preserved (no transcode), a single rectangular crop region. The crop math
//! routes through the shared EXIF-aware [`crate::image::decode_oriented`] so
//! coordinates are in the upright pixel space the user sees.

mod convert;

pub use convert::{crop_one, CropRect, PixelRect};
