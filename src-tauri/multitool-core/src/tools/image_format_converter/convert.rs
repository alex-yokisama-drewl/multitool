//! Image format converter — single-file pure transform.
//!
//! [`convert_one`] is the per-file shape: `(source_ext, bytes, opts)` in,
//! [`EncodedFile`] out. The batch orchestrator in [`super::job`] drives this
//! in a skip + continue loop — so a per-file failure here translates into a
//! per-file skip there, not a job-level abort.
//!
//! EXIF orientation is honored on input via [`decode_with_orientation`]
//! (mirrors the helper in `images_to_pdf::convert`; Phase F will extract the
//! shared body to `multitool_core::image` once three call sites agree).
//!
//! Per-tool spec: `docs/tools/image-format-converter.md`. Build plan:
//! `docs/tools/image-format-converter-plan.md`. Both delete on ship.

use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader};
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
    /// **Lossless only** — `image` 0.25's WebP encoder is lossless. Adding a
    /// lossy lane would require the `webp` crate (libwebp C bindings) and a
    /// native dep, deferred to a future commit if requested.
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

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Webp => ImageFormat::WebP,
            Self::Bmp => ImageFormat::Bmp,
            Self::Tiff => ImageFormat::Tiff,
        }
    }
}

/// How to handle alpha when the target format can't carry it (JPEG, BMP).
/// No-op for alpha-supporting targets. Wiring is in Phase A3.
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

/// Pixel size policy for SVG inputs. Ignored for raster inputs. Wiring lands
/// in Phase A4.
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
/// `jpeg_quality` is **clamped** inside `convert_one` to defend against buggy
/// or hostile callers; the UI clamps too for UX. WebP output is lossless only
/// (see [`TargetFormat::Webp`]) so there is no `webp_quality` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Opts {
    pub target_format: TargetFormat,
    /// 1..=100. Used when `target_format == Jpeg`.
    pub jpeg_quality: u8,
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

/// Inclusive bounds for `jpeg_quality`. Anything outside is silently moved to
/// the nearest bound — same DPI-clamp policy used by `pdf_to_images`.
pub(crate) const QUALITY_MIN: u8 = 1;
pub(crate) const QUALITY_MAX: u8 = 100;

/// Convert a single image's bytes from its source format to `opts.target_format`.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot) —
/// used only to route SVG inputs to the resvg path (`.svg`); everything else
/// goes through `image::ImageReader::with_guessed_format`, which sniffs the
/// raster format from bytes regardless of extension.
///
/// Errors are per-file. The batch orchestrator translates them into "skipped"
/// entries, so `convert_one` never has to think about job-level state.
///
/// Cancellation is the orchestrator's responsibility; this fn is a small,
/// CPU-bound transform without internal cancel checkpoints.
pub fn convert_one(source_ext: &str, input_bytes: &[u8], opts: &Opts) -> AppResult<EncodedFile> {
    if source_ext.eq_ignore_ascii_case("svg") {
        // SVG path lands in Phase A4 via resvg. Until then it surfaces as a
        // typed UnsupportedFormat so the orchestrator routes it through
        // skip + continue instead of aborting the whole batch.
        return Err(AppError::UnsupportedFormat {
            detail: "SVG input not yet wired (Phase A4 will rasterize via resvg)".into(),
        });
    }

    let img = decode_with_orientation(source_ext, input_bytes)?;
    encode_raster(&img, opts)
}

/// Decode `bytes` and apply any EXIF orientation tag the decoder exposes.
///
/// A decoder without an orientation tag (PNG without `eXIf`, JPEG saved
/// without orientation, …) is treated as `NoTransforms` — a missing tag never
/// fails a valid image.
///
/// `source_ext` is consulted only when bytes-sniffing comes back inconclusive
/// — TGA in particular has no reliable magic and `with_guessed_format` returns
/// no format for it. Trusting bytes first means a `.tga` file containing PNG
/// data still gets the right decoder; the extension is the fallback, not the
/// override.
///
/// Duplicated from `images_to_pdf::convert::decode_with_orientation` (with the
/// extension-fallback added here); Phase F will extract the shared body to
/// `multitool_core::image`.
fn decode_with_orientation(source_ext: &str, bytes: &[u8]) -> AppResult<DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("decode: {err}"),
        })?;
    if reader.format().is_none() {
        if let Some(fmt) = ImageFormat::from_extension(source_ext) {
            reader.set_format(fmt);
        }
    }
    let mut decoder = reader
        .into_decoder()
        .map_err(|err| image_to_app_err(Path::new(""), err))?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img =
        DynamicImage::from_decoder(decoder).map_err(|err| image_to_app_err(Path::new(""), err))?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Encode a decoded `DynamicImage` to the target raster format.
///
/// Alpha-handling-aware encode lands in Phase A3 — for now JPEG and BMP go
/// through `to_rgb8()`, which drops alpha. That's fine for the A2 test
/// fixtures (all opaque RGB) but would produce wrong colors for transparent
/// inputs; A3 routes those through the alpha matrix or skips them.
fn encode_raster(img: &DynamicImage, opts: &Opts) -> AppResult<EncodedFile> {
    let mut bytes = Vec::new();
    let warnings = Vec::new();

    match opts.target_format {
        TargetFormat::Jpeg => {
            // JPEG needs RGB and a quality parameter; image::write_to defaults
            // to quality=75, so we go through the encoder directly.
            let q = opts.jpeg_quality.clamp(QUALITY_MIN, QUALITY_MAX);
            let rgb = img.to_rgb8();
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, q);
            encoder
                .encode_image(&rgb)
                .map_err(|err| image_to_app_err(Path::new(""), err))?;
        }
        format => {
            // PNG / WebP (lossless) / BMP / TIFF all go through write_to.
            // Alpha handling for BMP is approximate in A2 — covered in A3.
            img.write_to(&mut Cursor::new(&mut bytes), format.image_format())
                .map_err(|err| image_to_app_err(Path::new(""), err))?;
        }
    }

    Ok(EncodedFile { bytes, warnings })
}

/// Map an `image::ImageError` into the right `AppError` variant. `path` is
/// included verbatim in the detail string when non-empty so the caller can
/// identify which file failed — empty paths are stripped to avoid noisy
/// `: ` prefixes in convert_one's path-less callers.
fn image_to_app_err(path: &Path, err: ImageError) -> AppError {
    let prefix = if path.as_os_str().is_empty() {
        String::new()
    } else {
        format!("{}: ", path.display())
    };
    match err {
        ImageError::Unsupported(_) | ImageError::Decoding(_) => AppError::UnsupportedFormat {
            detail: format!("{prefix}{err}"),
        },
        _ => AppError::ProcessingFailed {
            detail: format!("{prefix}{err}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn images_fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(format!("tests/fixtures/images/{name}"));
        fs::read(&path).unwrap_or_else(|_| panic!("read images fixture {name}: {path:?}"))
    }

    fn format_fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(format!("tests/fixtures/image_format/{name}"));
        fs::read(&path).unwrap_or_else(|_| panic!("read image_format fixture {name}: {path:?}"))
    }

    fn opts(target_format: TargetFormat) -> Opts {
        Opts {
            target_format,
            jpeg_quality: 85,
            alpha_handling: AlphaHandling::FlattenWhite,
            svg_raster_size: SvgRasterSize::LongestEdgePx(1024),
        }
    }

    /// Re-decode the encoded output and assert it round-trips back to the
    /// right format with the source dimensions intact.
    fn assert_round_trip(encoded: &[u8], expected_format: ImageFormat, w: u32, h: u32) {
        let reader = ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .expect("guess format on encoded output");
        assert_eq!(reader.format(), Some(expected_format));
        let img = reader.decode().expect("decode encoded output");
        assert_eq!(img.width(), w);
        assert_eq!(img.height(), h);
    }

    // --- Output-format coverage from a PNG source (covers all 5 targets) ---

    #[test]
    fn png_input_to_png_output_round_trips() {
        let out = convert_one("png", &images_fixture("red.png"), &opts(TargetFormat::Png))
            .expect("convert PNG → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn png_input_to_jpeg_output_round_trips() {
        let out = convert_one("png", &images_fixture("red.png"), &opts(TargetFormat::Jpeg))
            .expect("convert PNG → JPEG");
        assert_round_trip(&out.bytes, ImageFormat::Jpeg, 100, 50);
    }

    #[test]
    fn png_input_to_webp_output_round_trips_lossless() {
        let out = convert_one("png", &images_fixture("red.png"), &opts(TargetFormat::Webp))
            .expect("convert PNG → WebP");
        assert_round_trip(&out.bytes, ImageFormat::WebP, 100, 50);
    }

    #[test]
    fn png_input_to_bmp_output_round_trips() {
        let out = convert_one("png", &images_fixture("red.png"), &opts(TargetFormat::Bmp))
            .expect("convert PNG → BMP");
        assert_round_trip(&out.bytes, ImageFormat::Bmp, 100, 50);
    }

    #[test]
    fn png_input_to_tiff_output_round_trips() {
        let out = convert_one("png", &images_fixture("red.png"), &opts(TargetFormat::Tiff))
            .expect("convert PNG → TIFF");
        assert_round_trip(&out.bytes, ImageFormat::Tiff, 100, 50);
    }

    // --- Input-format coverage (each accepted decoder lands a PNG) ---

    #[test]
    fn jpeg_input_decodes_through_to_png() {
        let out = convert_one("jpg", &images_fixture("blue.jpg"), &opts(TargetFormat::Png))
            .expect("convert JPEG → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn webp_input_decodes_through_to_png() {
        let out = convert_one(
            "webp",
            &images_fixture("green.webp"),
            &opts(TargetFormat::Png),
        )
        .expect("convert WebP → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn bmp_input_decodes_through_to_png() {
        let out = convert_one("bmp", &format_fixture("tiny.bmp"), &opts(TargetFormat::Png))
            .expect("convert BMP → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn tiff_input_decodes_through_to_png() {
        let out = convert_one("tif", &format_fixture("tiny.tif"), &opts(TargetFormat::Png))
            .expect("convert TIFF → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn gif_input_decodes_through_to_png() {
        let out = convert_one("gif", &format_fixture("tiny.gif"), &opts(TargetFormat::Png))
            .expect("convert GIF → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn ico_input_decodes_through_to_png() {
        // ICO is a container with N embedded images; the image crate picks the
        // largest. Our fixture is 32×16, scaled-down from red.png.
        let out = convert_one("ico", &format_fixture("tiny.ico"), &opts(TargetFormat::Png))
            .expect("convert ICO → PNG");
        let reader = ImageReader::new(Cursor::new(&out.bytes))
            .with_guessed_format()
            .expect("re-read encoded ICO");
        assert_eq!(reader.format(), Some(ImageFormat::Png));
        let img = reader.decode().expect("decode encoded output");
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[test]
    fn tga_input_decodes_through_to_png() {
        let out = convert_one("tga", &format_fixture("tiny.tga"), &opts(TargetFormat::Png))
            .expect("convert TGA → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn pnm_input_decodes_through_to_png() {
        let out = convert_one("pgm", &format_fixture("tiny.pgm"), &opts(TargetFormat::Png))
            .expect("convert PGM → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn qoi_input_decodes_through_to_png() {
        let out = convert_one("qoi", &format_fixture("tiny.qoi"), &opts(TargetFormat::Png))
            .expect("convert QOI → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    // --- EXIF orientation honored on input ---

    #[test]
    fn exif_orientation_six_swaps_image_dimensions() {
        // rotated.jpg is encoded 100×50 with EXIF orientation 6 ("rotate 90
        // CW"). After apply_orientation, the image is 50×100, so the encoded
        // PNG carries the swapped dims.
        let out = convert_one(
            "jpg",
            &images_fixture("rotated.jpg"),
            &opts(TargetFormat::Png),
        )
        .expect("convert oriented JPEG → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 50, 100);
    }

    // --- JPEG quality clamp ---

    #[test]
    fn jpeg_quality_zero_is_clamped_silently_to_one() {
        // The encoder accepts 1..=100; a literal 0 would panic the encoder
        // or fail. The clamp guards against buggy callers.
        let mut o = opts(TargetFormat::Jpeg);
        o.jpeg_quality = 0;
        let out = convert_one("png", &images_fixture("red.png"), &o)
            .expect("convert at quality=0 (clamped to 1)");
        // Verify it still produces a valid JPEG (just very small).
        assert_round_trip(&out.bytes, ImageFormat::Jpeg, 100, 50);
    }

    #[test]
    fn jpeg_quality_high_produces_larger_output_than_low() {
        // Sanity check that the quality knob actually affects output size.
        let mut o_lo = opts(TargetFormat::Jpeg);
        o_lo.jpeg_quality = 10;
        let mut o_hi = opts(TargetFormat::Jpeg);
        o_hi.jpeg_quality = 95;

        let lo = convert_one("png", &images_fixture("red.png"), &o_lo).expect("lo q");
        let hi = convert_one("png", &images_fixture("red.png"), &o_hi).expect("hi q");
        assert!(
            hi.bytes.len() > lo.bytes.len(),
            "expected hi-quality JPEG ({}) > lo-quality JPEG ({})",
            hi.bytes.len(),
            lo.bytes.len()
        );
    }

    // --- Error cases ---

    #[test]
    fn unsupported_bytes_yield_unsupported_format() {
        let out = convert_one(
            "bin",
            &images_fixture("garbage.bin"),
            &opts(TargetFormat::Png),
        );
        match out {
            Err(AppError::UnsupportedFormat { .. }) => {}
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    #[test]
    fn svg_extension_returns_unsupported_until_phase_a4() {
        let out = convert_one("svg", b"<svg/>", &opts(TargetFormat::Png));
        match out {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(detail.contains("Phase A4"), "detail was {detail}");
            }
            other => panic!("expected UnsupportedFormat for SVG, got {other:?}"),
        }
    }

    #[test]
    fn empty_bytes_yield_unsupported_format() {
        let out = convert_one("png", &[], &opts(TargetFormat::Png));
        match out {
            Err(AppError::UnsupportedFormat { .. } | AppError::ProcessingFailed { .. }) => {}
            other => panic!("expected Unsupported/ProcessingFailed for empty bytes, got {other:?}"),
        }
    }
}
