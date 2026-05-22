//! Image format converter — single-file pure transform.
//!
//! [`convert_one`] is the per-file shape: `(source_ext, bytes, opts)` in,
//! [`EncodedFile`] out. The batch orchestrator in [`super::job`] drives this
//! in a skip + continue loop — so a per-file failure here translates into a
//! per-file skip there, not a job-level abort.
//!
//! EXIF orientation is honored on input via
//! [`multitool_core::image::decode_oriented`], the shared helper that
//! `images_to_pdf` also routes through.
//!
//! Per-tool spec: `docs/tools/image-format-converter.md`. Build plan:
//! `docs/tools/image-format-converter-plan.md`. Both delete on ship.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
#[cfg(test)]
use image::ImageReader;
use image::{AnimationDecoder, DynamicImage, ImageFormat, RgbImage};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::image::{decode_oriented, image_to_app_err};

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
/// No-op for alpha-supporting targets. Implementation in
/// [`apply_alpha_handling`] / [`flatten_onto`].
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

/// Inclusive bounds for `SvgRasterSize::LongestEdgePx`. Above this the
/// allocated pixmap balloons (8192² × 4 bytes ≈ 256 MB) — past the
/// "interactive desktop tool" use case and into "did you mean to do that?"
/// territory. Silently clamped to the nearest bound, matching the
/// `jpeg_quality` policy.
pub(crate) const SVG_PX_MIN: u32 = 1;
pub(crate) const SVG_PX_MAX: u32 = 8192;

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
    let mut warnings: Vec<String> = Vec::new();
    let img = if source_ext.eq_ignore_ascii_case("svg") {
        rasterize_svg(input_bytes, opts.svg_raster_size, &mut warnings)?
    } else {
        if source_ext.eq_ignore_ascii_case("gif") && is_animated_gif(input_bytes) {
            warnings.push("animated GIF; converted first frame only".into());
        }
        decode_oriented(source_ext, input_bytes)?
    };
    let mut encoded = encode_raster(&img, opts)?;
    encoded.warnings = warnings;
    Ok(encoded)
}

/// Cheap check for whether `bytes` contain more than one GIF frame. Returns
/// `false` for non-GIF inputs, malformed GIFs, or single-frame GIFs.
///
/// Uses `image::AnimationDecoder::into_frames().take(2).count()` — decodes
/// at most two frames' pixel data, which is acceptable for the small GIFs
/// users typically pick. A byte-level magic + Image-Descriptor scan would
/// be faster but isn't worth the parser maintenance burden for a per-file
/// detection that's already O(1) in the file count.
fn is_animated_gif(bytes: &[u8]) -> bool {
    use image::codecs::gif::GifDecoder;
    GifDecoder::new(Cursor::new(bytes))
        .map(|d| d.into_frames().take(2).count() > 1)
        .unwrap_or(false)
}

/// Rasterize an SVG document to a `DynamicImage::ImageRgba8` via `resvg`.
///
/// Size policy:
/// - [`SvgRasterSize::Natural`] uses `usvg`'s reported tree size, which
///   honors `width`/`height` first, falls back to `viewBox`, and finally
///   to its own default if neither is present. We don't second-guess usvg
///   here — a document that resolves to a non-positive size on its own is
///   the only thing we reject as `UnsupportedFormat`.
/// - [`SvgRasterSize::LongestEdgePx`] scales so the longest side is exactly
///   the requested pixel count (clamped to `[SVG_PX_MIN, SVG_PX_MAX]`),
///   preserving aspect ratio.
///
/// Errors:
/// - `AppError::UnsupportedFormat` if `usvg` rejects the bytes or returns
///   a non-positive / non-finite size.
/// - `AppError::ProcessingFailed` if the target pixmap can't be allocated.
///
/// Fonts: an empty `fontdb` is passed; text nodes parse and route through
/// usvg's text pipeline but render glyphless (no fonts loaded). A per-file
/// warning is appended to `warnings` when text nodes are detected so the UI
/// can flag "text may not render".
fn rasterize_svg(
    bytes: &[u8],
    size_policy: SvgRasterSize,
    warnings: &mut Vec<String>,
) -> AppResult<DynamicImage> {
    // Detect text elements BEFORE handing bytes to usvg — usvg with the
    // `text` feature may convert `<text>` to paths during parse (so the
    // resulting tree no longer has a `Node::Text` variant). A simple byte
    // scan is independent of usvg's internal representation. False
    // positives are tolerated: the warning is informational.
    if svg_bytes_contain_text_element(bytes) {
        warnings.push("SVG references fonts; text may not render".into());
    }
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|err| {
        AppError::UnsupportedFormat {
            detail: format!("SVG parse: {err}"),
        }
    })?;
    let intrinsic = tree.size();
    let intrinsic_w = intrinsic.width();
    let intrinsic_h = intrinsic.height();
    if intrinsic_w <= 0.0
        || intrinsic_h <= 0.0
        || !intrinsic_w.is_finite()
        || !intrinsic_h.is_finite()
    {
        return Err(AppError::UnsupportedFormat {
            detail: "SVG has no usable dimensions (need width/height or viewBox)".into(),
        });
    }

    let (target_w, target_h, scale) = match size_policy {
        SvgRasterSize::Natural => (
            intrinsic_w.round().max(1.0) as u32,
            intrinsic_h.round().max(1.0) as u32,
            1.0_f32,
        ),
        SvgRasterSize::LongestEdgePx(requested) => {
            let n = requested.clamp(SVG_PX_MIN, SVG_PX_MAX) as f32;
            let longest = intrinsic_w.max(intrinsic_h);
            let scale = n / longest;
            (
                (intrinsic_w * scale).round().max(1.0) as u32,
                (intrinsic_h * scale).round().max(1.0) as u32,
                scale,
            )
        }
    };

    let mut pixmap = resvg::tiny_skia::Pixmap::new(target_w, target_h).ok_or_else(|| {
        AppError::ProcessingFailed {
            detail: format!("SVG: failed to allocate pixmap {target_w}×{target_h}"),
        }
    })?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(DynamicImage::ImageRgba8(pixmap_to_rgba_image(&pixmap)))
}

/// Heuristic: does the SVG source contain a `<text>` or `<tspan>` element?
///
/// Run against the raw bytes BEFORE handing them to `usvg`. The `text`
/// feature in usvg converts text elements to paths during parse (when the
/// `fontdb` permits), so by the time the parsed `Tree` is in hand the text
/// nodes have been lowered and aren't directly visible. Byte-scanning the
/// source keeps the detection independent of usvg's lowering pass.
///
/// Tolerates false positives (e.g. matches inside a CDATA / comment) — the
/// warning is informational, not load-bearing.
fn svg_bytes_contain_text_element(bytes: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(bytes) else {
        return false;
    };
    // `<text ` / `<text>` / `<tspan ` / `<tspan>`. Cheap-enough sequential
    // scan; SVGs are typically a few KB.
    s.contains("<text ") || s.contains("<text>") || s.contains("<tspan ") || s.contains("<tspan>")
}

/// Convert a `resvg::tiny_skia::Pixmap` (RGBA8, premultiplied) into an
/// `image::RgbaImage` (RGBA8, non-premultiplied). `demultiply()` is the
/// official tiny_skia helper for this — un-premultiplied pixels are what
/// `image`'s encoders expect.
fn pixmap_to_rgba_image(pixmap: &resvg::tiny_skia::Pixmap) -> image::RgbaImage {
    let w = pixmap.width();
    let h = pixmap.height();
    let mut img = image::RgbaImage::new(w, h);
    for (px_in, px_out) in pixmap.pixels().iter().zip(img.pixels_mut()) {
        let c = px_in.demultiply();
        px_out.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    img
}

/// Encode a decoded `DynamicImage` to the target raster format.
///
/// For alpha-supporting targets (PNG / WebP / TIFF) `alpha_handling` is a
/// no-op — the source alpha rides through unchanged. For alpha-less targets
/// (JPEG / BMP) we consult [`apply_alpha_handling`] up front: `Preserve`
/// fails the file (skipped at the orchestrator layer); the `Flatten*` modes
/// composite RGBA onto a solid background and drop alpha.
fn encode_raster(img: &DynamicImage, opts: &Opts) -> AppResult<EncodedFile> {
    let mut bytes = Vec::new();
    let warnings = Vec::new();
    let flattened = apply_alpha_handling(img, opts)?;
    // `flattened` is `Some` only when we composited onto a background — the
    // result is RGB-only and replaces the RGB pixels going into the encoder.
    // For PNG / WebP / TIFF, `flattened` is always `None` and `img` flows
    // through unchanged (alpha preserved).
    match opts.target_format {
        TargetFormat::Jpeg => {
            let q = opts.jpeg_quality.clamp(QUALITY_MIN, QUALITY_MAX);
            // If `apply_alpha_handling` returned a flattened buffer, use it;
            // otherwise the source is already RGB (or RGB-like) so to_rgb8
            // is a cheap conversion that doesn't lose information.
            let rgb = flattened.unwrap_or_else(|| img.to_rgb8());
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, q);
            encoder.encode_image(&rgb).map_err(image_to_app_err)?;
        }
        TargetFormat::Bmp => {
            // BMP encoder in `image` 0.25 accepts RGBA but doesn't store it;
            // route flattened RGB through `DynamicImage::ImageRgb8` so the
            // alpha-handling intent shows up in the encoded bytes.
            let bmp_img = match flattened {
                Some(rgb) => DynamicImage::ImageRgb8(rgb),
                None => img.clone(),
            };
            bmp_img
                .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Bmp)
                .map_err(image_to_app_err)?;
        }
        format => {
            // PNG / WebP (lossless) / TIFF — alpha-supporting targets.
            img.write_to(&mut Cursor::new(&mut bytes), format.image_format())
                .map_err(image_to_app_err)?;
        }
    }

    Ok(EncodedFile { bytes, warnings })
}

/// Resolve the alpha-handling policy for an alpha-less target.
///
/// Returns:
/// - `Ok(None)` when no action is needed:
///   - the target supports alpha (PNG / WebP / TIFF), or
///   - the source has no alpha channel, or
///   - the source's alpha channel is fully opaque (every pixel α = 255).
/// - `Ok(Some(RgbImage))` when an alpha-less target needs the source RGBA
///   composited onto a solid background (`FlattenWhite` / `FlattenBlack`).
/// - `Err(AppError::ProcessingFailed)` when `Preserve` would silently drop
///   non-trivial alpha — explicit refusal so the orchestrator can skip the
///   file with a clear reason.
fn apply_alpha_handling(img: &DynamicImage, opts: &Opts) -> AppResult<Option<RgbImage>> {
    if opts.target_format.supports_alpha() {
        return Ok(None);
    }
    if !has_non_trivial_alpha(img) {
        // Source is effectively opaque; `to_rgb8` is lossless on the visible
        // channels. No need to walk pixels.
        return Ok(None);
    }
    match opts.alpha_handling {
        AlphaHandling::Preserve => Err(AppError::ProcessingFailed {
            detail: "target format does not support alpha; choose a flatten option".into(),
        }),
        AlphaHandling::FlattenWhite => Ok(Some(flatten_onto(img, [255, 255, 255]))),
        AlphaHandling::FlattenBlack => Ok(Some(flatten_onto(img, [0, 0, 0]))),
    }
}

/// True iff `img` has an alpha channel AND at least one pixel is not fully
/// opaque. A fully-opaque RGBA image returns `false` so we don't pay the
/// flatten cost (or refuse a Preserve-mode encode) for what is effectively
/// an RGB image.
fn has_non_trivial_alpha(img: &DynamicImage) -> bool {
    if !img.color().has_alpha() {
        return false;
    }
    img.to_rgba8().pixels().any(|p| p.0[3] != 255)
}

/// Composite the source RGBA onto a solid `[r, g, b]` background, returning
/// an `RgbImage`. Standard non-premultiplied alpha:
///   out = (α · src + (255 − α) · bg) / 255
fn flatten_onto(img: &DynamicImage, bg: [u8; 3]) -> RgbImage {
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (px_in, px_out) in rgba.pixels().zip(out.pixels_mut()) {
        let [r, g, b, a] = px_in.0;
        let a = u16::from(a);
        let inv_a = 255 - a;
        px_out.0 = [
            (((u16::from(r) * a) + (u16::from(bg[0]) * inv_a)) / 255) as u8,
            (((u16::from(g) * a) + (u16::from(bg[1]) * inv_a)) / 255) as u8,
            (((u16::from(b) * a) + (u16::from(bg[2]) * inv_a)) / 255) as u8,
        ];
    }
    out
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

    // --- SVG input (Phase A4) ---

    #[test]
    fn svg_natural_size_honors_intrinsic_dimensions() {
        // tiny.svg declares width="100" height="50", viewBox=0 0 100 50.
        let mut o = opts(TargetFormat::Png);
        o.svg_raster_size = SvgRasterSize::Natural;
        let out =
            convert_one("svg", &format_fixture("tiny.svg"), &o).expect("SVG → PNG at natural size");
        assert_round_trip(&out.bytes, ImageFormat::Png, 100, 50);
    }

    #[test]
    fn svg_longest_edge_px_scales_aspect_preserved() {
        // tiny.svg is 100×50 (2:1). longest-edge=200 → output is 200×100.
        let mut o = opts(TargetFormat::Png);
        o.svg_raster_size = SvgRasterSize::LongestEdgePx(200);
        let out = convert_one("svg", &format_fixture("tiny.svg"), &o)
            .expect("SVG → PNG at longest-edge 200");
        assert_round_trip(&out.bytes, ImageFormat::Png, 200, 100);
    }

    #[test]
    fn svg_longest_edge_px_below_min_is_clamped() {
        // 0 (below SVG_PX_MIN=1) clamps to 1; 100×50 SVG → 1×0 after rounding,
        // but the .max(1.0) floor keeps both axes ≥ 1.
        let mut o = opts(TargetFormat::Png);
        o.svg_raster_size = SvgRasterSize::LongestEdgePx(0);
        let out =
            convert_one("svg", &format_fixture("tiny.svg"), &o).expect("SVG → PNG clamped to 1px");
        let reader = ImageReader::new(Cursor::new(&out.bytes))
            .with_guessed_format()
            .expect("re-read");
        let img = reader.decode().expect("decode");
        assert!(img.width() >= 1 && img.height() >= 1);
        assert!(img.width() <= 1, "width should be clamped: {}", img.width());
    }

    #[test]
    fn svg_renders_red_rectangle_visible_in_output() {
        // Quick sanity check that resvg actually produced pixels, not an
        // empty pixmap. Center pixel of the encoded PNG should be red-ish.
        let mut o = opts(TargetFormat::Png);
        o.svg_raster_size = SvgRasterSize::Natural;
        let out = convert_one("svg", &format_fixture("tiny.svg"), &o).expect("rasterize tiny.svg");
        let decoded = decode_png(&out.bytes).to_rgba8();
        let center = decoded.get_pixel(50, 25).0;
        assert_eq!(
            center,
            [255, 0, 0, 255],
            "expected solid red at center, got {center:?}"
        );
    }

    #[test]
    fn svg_to_jpeg_routes_through_alpha_flatten() {
        // SVG is RGBA from resvg. JPEG can't carry alpha → FlattenWhite
        // composites onto white. The tiny.svg covers the whole frame in
        // red, so no transparent area exists — just verify the path runs.
        let mut o = opts(TargetFormat::Jpeg);
        o.svg_raster_size = SvgRasterSize::Natural;
        o.alpha_handling = AlphaHandling::FlattenWhite;
        let out = convert_one("svg", &format_fixture("tiny.svg"), &o)
            .expect("SVG → JPEG via alpha flatten");
        assert_round_trip(&out.bytes, ImageFormat::Jpeg, 100, 50);
    }

    // --- Per-file warnings (Phase A6) ---

    #[test]
    fn animated_gif_input_emits_first_frame_warning() {
        let out = convert_one(
            "gif",
            &format_fixture("animated.gif"),
            &opts(TargetFormat::Png),
        )
        .expect("animated GIF → PNG");
        assert_round_trip(&out.bytes, ImageFormat::Png, 8, 8);
        assert!(
            out.warnings.iter().any(|w| w.contains("animated GIF")),
            "expected animated-GIF warning, got {:?}",
            out.warnings
        );
    }

    #[test]
    fn static_gif_input_emits_no_animation_warning() {
        let out = convert_one("gif", &format_fixture("tiny.gif"), &opts(TargetFormat::Png))
            .expect("static GIF → PNG");
        assert!(
            out.warnings.is_empty(),
            "static GIF should have no warnings, got {:?}",
            out.warnings
        );
    }

    #[test]
    fn svg_with_text_emits_font_warning() {
        let mut o = opts(TargetFormat::Png);
        o.svg_raster_size = SvgRasterSize::Natural;
        let out =
            convert_one("svg", &format_fixture("tiny-text.svg"), &o).expect("SVG with text → PNG");
        assert!(
            out.warnings
                .iter()
                .any(|w| w.contains("text may not render")),
            "expected SVG-text warning, got {:?}",
            out.warnings
        );
    }

    #[test]
    fn svg_without_text_emits_no_warning() {
        let mut o = opts(TargetFormat::Png);
        o.svg_raster_size = SvgRasterSize::Natural;
        let out = convert_one("svg", &format_fixture("tiny.svg"), &o).expect("plain SVG → PNG");
        assert!(
            out.warnings.is_empty(),
            "plain SVG should have no warnings, got {:?}",
            out.warnings
        );
    }

    #[test]
    fn malformed_svg_yields_unsupported_format() {
        let result = convert_one("svg", b"not an svg", &opts(TargetFormat::Png));
        match result {
            Err(AppError::UnsupportedFormat { .. }) => {}
            other => panic!("expected UnsupportedFormat for malformed SVG, got {other:?}"),
        }
    }

    // --- Alpha handling matrix (Phase A3) ---

    fn decode_png(bytes: &[u8]) -> DynamicImage {
        ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .expect("guess format")
            .decode()
            .expect("decode")
    }

    #[test]
    fn alpha_preserved_when_target_supports_alpha() {
        // alpha.png is 4×4: left half opaque red (255,0,0,255), right half
        // fully transparent (0,0,0,0). PNG output keeps it byte-for-byte
        // equivalent (alpha channel intact, transparent right half).
        let out = convert_one(
            "png",
            &format_fixture("alpha.png"),
            &opts(TargetFormat::Png),
        )
        .expect("PNG → PNG with alpha");
        let decoded = decode_png(&out.bytes);
        let rgba = decoded.to_rgba8();
        assert_eq!(
            rgba.get_pixel(0, 0).0,
            [255, 0, 0, 255],
            "left = opaque red"
        );
        assert_eq!(
            rgba.get_pixel(3, 0).0,
            [0, 0, 0, 0],
            "right = fully transparent"
        );
    }

    #[test]
    fn preserve_against_jpeg_refuses_alpha_image() {
        let mut o = opts(TargetFormat::Jpeg);
        o.alpha_handling = AlphaHandling::Preserve;
        let result = convert_one("png", &format_fixture("alpha.png"), &o);
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(
                    detail.contains("alpha"),
                    "expected alpha-related detail, got {detail}"
                );
            }
            other => panic!("expected ProcessingFailed for preserve+JPEG, got {other:?}"),
        }
    }

    #[test]
    fn flatten_white_composites_transparent_pixels_to_white() {
        // Right half (α=0) becomes white; left half (α=255) stays red.
        let mut o = opts(TargetFormat::Jpeg);
        o.alpha_handling = AlphaHandling::FlattenWhite;
        // Use high quality so the round-trip preserves colors closely enough
        // for byte-level assertions.
        o.jpeg_quality = 100;
        let out = convert_one("png", &format_fixture("alpha.png"), &o)
            .expect("convert with flatten-white");
        let decoded = decode_png(&out.bytes).to_rgb8();
        // JPEG is lossy even at q=100, so allow a tolerance.
        let left = decoded.get_pixel(0, 0).0;
        let right = decoded.get_pixel(3, 0).0;
        assert!(
            left[0] > 200 && left[1] < 60 && left[2] < 60,
            "left should look red, got {left:?}"
        );
        assert!(
            right[0] > 240 && right[1] > 240 && right[2] > 240,
            "right should look white, got {right:?}"
        );
    }

    #[test]
    fn flatten_black_composites_transparent_pixels_to_black() {
        let mut o = opts(TargetFormat::Jpeg);
        o.alpha_handling = AlphaHandling::FlattenBlack;
        o.jpeg_quality = 100;
        let out = convert_one("png", &format_fixture("alpha.png"), &o)
            .expect("convert with flatten-black");
        let decoded = decode_png(&out.bytes).to_rgb8();
        let left = decoded.get_pixel(0, 0).0;
        let right = decoded.get_pixel(3, 0).0;
        assert!(
            left[0] > 200 && left[1] < 60 && left[2] < 60,
            "left should look red, got {left:?}"
        );
        assert!(
            right[0] < 15 && right[1] < 15 && right[2] < 15,
            "right should look black, got {right:?}"
        );
    }

    #[test]
    fn flatten_white_against_bmp_writes_rgb_bmp() {
        let mut o = opts(TargetFormat::Bmp);
        o.alpha_handling = AlphaHandling::FlattenWhite;
        let out = convert_one("png", &format_fixture("alpha.png"), &o)
            .expect("convert alpha-PNG → BMP with flatten-white");
        assert_round_trip(&out.bytes, ImageFormat::Bmp, 4, 4);
    }

    #[test]
    fn preserve_against_jpeg_with_opaque_source_succeeds() {
        // No alpha to preserve → no refusal. Sanity-checks that the
        // "non-trivial alpha" gate doesn't trip on plain RGB inputs.
        let mut o = opts(TargetFormat::Jpeg);
        o.alpha_handling = AlphaHandling::Preserve;
        let out = convert_one("png", &images_fixture("red.png"), &o)
            .expect("preserve on opaque RGB → JPEG");
        assert_round_trip(&out.bytes, ImageFormat::Jpeg, 100, 50);
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
