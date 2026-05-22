//! Image format converter — single-file pure transform.
//!
//! [`convert_one`] is the per-file shape: `(source_ext, bytes, opts)` in,
//! [`EncodedFile`] out. The batch orchestrator in [`super::job`] drives this
//! in a skip + continue loop — so a per-file failure here translates into a
//! per-file skip there, not a job-level abort.
//!
//! Per-tool spec: `docs/tools/image-format-converter.md`. Build plan:
//! `docs/tools/image-format-converter-plan.md`. Both delete on ship.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Raster format the converter emits. Vector outputs are intentionally out of
/// scope (the brief explicitly rejects "fake" raster→vector conversions like
/// PNG → SVG tracing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetFormat {
    Png,
    Jpeg,
    Webp,
    Bmp,
    Tiff,
}

impl TargetFormat {
    /// File extension (no leading dot) for output naming.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tif",
        }
    }

    /// True when the encoder preserves an alpha channel without flattening.
    /// JPEG and BMP have no alpha; PNG/WebP/TIFF do.
    pub fn supports_alpha(self) -> bool {
        matches!(self, Self::Png | Self::Webp | Self::Tiff)
    }
}

/// How to handle alpha when the target format can't carry it (JPEG, BMP).
/// No-op for alpha-supporting targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AlphaHandling {
    /// Skip the file when it has non-trivial alpha and the target can't carry
    /// it. Surfaces in the batch's skipped-files list.
    Preserve,
    /// Composite RGBA onto solid white before encoding.
    FlattenWhite,
    /// Composite RGBA onto solid black before encoding.
    FlattenBlack,
}

/// Pixel size policy for SVG inputs. Ignored for raster inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SvgRasterSize {
    /// Use the SVG's intrinsic `width`/`height` (or `viewBox` as fallback).
    Natural,
    /// Scale so the longest side is exactly N pixels (aspect preserved).
    LongestEdgePx(u32),
}

/// User-facing options. Mirrors the form fields on the tool view.
///
/// Quality fields and the SVG px field are **clamped** inside `convert_one` to
/// defend against buggy or hostile callers; the UI clamps too for UX.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Opts {
    pub target_format: TargetFormat,
    /// 1..=100. Used when `target_format == Jpeg`.
    pub jpeg_quality: u8,
    /// 1..=100. Used when `target_format == Webp`. `100` = lossless.
    pub webp_quality: u8,
    pub alpha_handling: AlphaHandling,
    pub svg_raster_size: SvgRasterSize,
}

/// Output of a single-file convert: encoded bytes plus any non-fatal warnings
/// raised while processing (e.g. animated GIF → first frame only, SVG text
/// nodes encountered, etc.). The orchestrator forwards warnings to the UI
/// without failing the file.
#[derive(Clone, Debug, Default)]
pub struct EncodedFile {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

/// Convert a single image's bytes from its source format to `opts.target_format`.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot) —
/// used only to route SVG inputs to the resvg path (`.svg`); everything else
/// goes through `image::ImageReader::with_guessed_format`, which sniffs the
/// raster format from bytes.
///
/// Errors are per-file. The batch orchestrator translates them into "skipped"
/// entries, so `convert_one` never has to think about job-level state.
///
/// Cancellation is the orchestrator's responsibility; this fn is a small,
/// CPU-bound transform without internal cancel checkpoints.
pub fn convert_one(_source_ext: &str, _input_bytes: &[u8], _opts: &Opts) -> AppResult<EncodedFile> {
    // Scaffold stub. A2 lands the raster path; A4 lands the SVG path.
    Err(AppError::ProcessingFailed {
        detail: "image-format-converter convert_one is a Phase-A1 scaffold stub".into(),
    })
}
