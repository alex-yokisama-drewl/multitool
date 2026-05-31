//! Image resize — single-image pure transform.
//!
//! [`resize_one`] decodes the source (EXIF-oriented), dispatches on the
//! requested [`FitMode`], and re-encodes **in the source extension's
//! format** — the same "preserve the source format" contract used by
//! [`super::super::image_crop`]. JPEG re-encodes at the shared
//! [`JPEG_REENCODE_QUALITY`]; the other formats ride through `write_to`.
//!
//! Dimensions are interpreted in the post-orientation pixel space (what the
//! user sees in the preview). The output is written upright with no EXIF
//! orientation tag.
//!
//! Clamping policy (see the working doc): silently fix recoverable inputs
//! (zero dims → 1px; over-MAX_DIM → MAX_DIM); reject only inputs with no
//! defensible interpretation (`Transparent` background on an alpha-less
//! target format; multi-frame TIFF; unsupported source extension; garbage
//! bytes).
//!
//! Fit-mode dispatch (see [`FitMode`]):
//! - **Stretch**: `resize_exact(W, H, Lanczos3)`. Source aspect ignored.
//! - **Contain**: scale source to fit inside the W×H box with aspect
//!   preserved, then composite onto a W×H canvas filled with the chosen
//!   background. Letterbox bars take the background color.
//! - **Cover**: scale source to fill the W×H box with aspect preserved,
//!   then center-crop the overflow back to W×H.
//!
//! When source and target aspects match, all three modes produce
//! pixel-equivalent results — there's no special case in the dispatcher.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::image::tiff::tiff_frame_count;
use crate::image::{decode_oriented, image_to_app_err, RasterFormat, JPEG_REENCODE_QUALITY};

/// How a resize maps the source pixels into the target W×H box when the
/// aspects differ. When the aspects match, all three modes collapse to a
/// plain proportional scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FitMode {
    /// Resize directly to W×H, ignoring the source aspect. May distort.
    Stretch,
    /// Aspect-preserved scale that fits **inside** the W×H box; the
    /// difference fills with the chosen [`Background`].
    Contain,
    /// Aspect-preserved scale that **fills** the W×H box; the overflow is
    /// center-cropped.
    Cover,
}

/// Color used for the letterbox bars in [`FitMode::Contain`]. Ignored by
/// `Stretch` and `Cover`. `Transparent` requires the target format to
/// support alpha (PNG / WebP / TIFF) — rejected up-front on JPEG / BMP.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Background {
    White,
    Black,
    Transparent,
}

/// User-facing options. Mirrors the form fields on the tool view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Opts {
    pub width: u32,
    pub height: u32,
    pub fit: FitMode,
    pub background: Background,
}

/// Inclusive bounds for output dimensions. The max defends against
/// pathological inputs that would balloon the encoder buffer past ≈ 1 GB
/// (16384² × 4 bytes). Silently clamped; matches the DPI-clamp / SVG-px
/// clamp policy elsewhere.
pub(crate) const MIN_DIM: u32 = 1;
pub(crate) const MAX_DIM: u32 = 16384;

/// Resize a single image's bytes to `opts.{width, height}` under `opts.fit`,
/// preserving the source format.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot).
/// It picks the **output** encoder, matching the crop tool's "keep the
/// extension" rule — a PNG-bytes-as-`.jpg` file re-encodes as JPEG. Bytes
/// always win on decode (`decode_oriented` sniffs the format).
///
/// Errors:
/// - `UnsupportedFormat` for: source extension not in [`RasterFormat`];
///   multi-frame TIFF; `Background::Transparent` + `FitMode::Contain` on a
///   target format without alpha support.
/// - decode/encode failures map through [`image_to_app_err`].
pub fn resize_one(source_ext: &str, input_bytes: &[u8], opts: &Opts) -> AppResult<Vec<u8>> {
    let target = RasterFormat::from_extension(source_ext).ok_or_else(|| {
        AppError::UnsupportedFormat {
            detail: format!(
                "image resize preserves the source format; '{source_ext}' is not an encodable raster format (png/jpg/webp/bmp/tif)"
            ),
        }
    })?;

    if let Some(frames) = tiff_frame_count(input_bytes) {
        if frames > 1 {
            return Err(AppError::UnsupportedFormat {
                detail: "multi-frame TIFF is not supported; only single-frame TIFFs can be resized while preserving the source format".into(),
            });
        }
    }

    // The Transparent-on-alpha-less-target combination is only ill-defined
    // when it would actually be used — i.e. when the fit mode synthesizes a
    // background. Stretch and Cover never look at `background`, so a stale
    // `Transparent` choice in those modes is ignored, matching how the UI
    // hides the background selector outside Contain.
    if matches!(opts.fit, FitMode::Contain)
        && matches!(opts.background, Background::Transparent)
        && !target.supports_alpha()
    {
        return Err(AppError::UnsupportedFormat {
            detail: format!(
                "target format '{}' does not support alpha; choose a white or black background for Contain",
                target.default_extension()
            ),
        });
    }

    let width = opts.width.clamp(MIN_DIM, MAX_DIM);
    let height = opts.height.clamp(MIN_DIM, MAX_DIM);

    let img = decode_oriented(source_ext, input_bytes)?;
    let (ws, hs) = (img.width(), img.height());

    let resized = match opts.fit {
        FitMode::Stretch => img.resize_exact(width, height, FilterType::Lanczos3),
        FitMode::Cover => resize_cover(&img, ws, hs, width, height),
        FitMode::Contain => resize_contain(&img, ws, hs, width, height, opts.background),
    };

    encode_to(&resized, target)
}

/// Aspect-preserved scale that fills the W×H box; overflow is
/// center-cropped. `scale = max(W/Ws, H/Hs)` guarantees the resized image
/// covers W×H in both dimensions; `ceil` + `.max(W/H)` defends against
/// rounding leaving the scaled extent 1px short.
fn resize_cover(img: &DynamicImage, ws: u32, hs: u32, w: u32, h: u32) -> DynamicImage {
    let scale = (f64::from(w) / f64::from(ws)).max(f64::from(h) / f64::from(hs));
    let scaled_w = ((f64::from(ws) * scale).ceil() as u32).max(w);
    let scaled_h = ((f64::from(hs) * scale).ceil() as u32).max(h);
    let resized = img.resize_exact(scaled_w, scaled_h, FilterType::Lanczos3);
    let x_off = (scaled_w - w) / 2;
    let y_off = (scaled_h - h) / 2;
    resized.crop_imm(x_off, y_off, w, h)
}

/// Aspect-preserved scale that fits inside the W×H box; the difference
/// fills with `bg`. The resized source goes through an RGBA buffer so
/// `imageops::overlay` can alpha-composite onto the canvas regardless of
/// background choice.
///
/// Output color type:
/// - `Transparent` → RGBA8 (letterbox bars carry α=0).
/// - `White` / `Black` → RGB8 (the canvas starts fully opaque, so dropping
///   alpha after the overlay is lossless and means the encoded output has
///   no transparency channel even on alpha-supporting formats).
fn resize_contain(
    img: &DynamicImage,
    ws: u32,
    hs: u32,
    w: u32,
    h: u32,
    bg: Background,
) -> DynamicImage {
    let scale = (f64::from(w) / f64::from(ws)).min(f64::from(h) / f64::from(hs));
    let scaled_w = ((f64::from(ws) * scale).round() as u32).clamp(MIN_DIM, w);
    let scaled_h = ((f64::from(hs) * scale).round() as u32).clamp(MIN_DIM, h);
    let resized_rgba = img
        .resize_exact(scaled_w, scaled_h, FilterType::Lanczos3)
        .into_rgba8();

    let x_off = i64::from((w - scaled_w) / 2);
    let y_off = i64::from((h - scaled_h) / 2);

    match bg {
        Background::Transparent => {
            let mut canvas = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
            image::imageops::overlay(&mut canvas, &resized_rgba, x_off, y_off);
            DynamicImage::ImageRgba8(canvas)
        }
        Background::White | Background::Black => {
            let c: u8 = if matches!(bg, Background::White) {
                255
            } else {
                0
            };
            let mut canvas = RgbaImage::from_pixel(w, h, Rgba([c, c, c, 255]));
            image::imageops::overlay(&mut canvas, &resized_rgba, x_off, y_off);
            // Canvas was fully opaque; overlay only composites within the
            // placed region, which itself blends over an opaque background.
            // The result is opaque everywhere, so dropping alpha is lossless
            // and lets us encode the output without a transparency channel.
            DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(canvas).to_rgb8())
        }
    }
}

/// Encode a (resized) image to `format`. JPEG goes through the fixed-quality
/// encoder over an RGB buffer; every other format rides through `write_to`,
/// preserving alpha for PNG/WebP/TIFF.
fn encode_to(img: &DynamicImage, format: RasterFormat) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    match format {
        RasterFormat::Jpeg => {
            let rgb = img.to_rgb8();
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, JPEG_REENCODE_QUALITY);
            encoder.encode_image(&rgb).map_err(image_to_app_err)?;
        }
        other => {
            img.write_to(&mut Cursor::new(&mut bytes), other.image_format())
                .map_err(image_to_app_err)?;
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, ImageReader, RgbaImage};
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

    fn opts(width: u32, height: u32, fit: FitMode, background: Background) -> Opts {
        Opts {
            width,
            height,
            fit,
            background,
        }
    }

    fn decode_to_rgba(encoded: &[u8]) -> RgbaImage {
        ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .expect("guess format")
            .decode()
            .expect("decode")
            .to_rgba8()
    }

    fn assert_round_trip(encoded: &[u8], expected: ImageFormat, w: u32, h: u32) {
        let reader = ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .expect("guess format on resized output");
        assert_eq!(reader.format(), Some(expected));
        let img = reader.decode().expect("decode resized output");
        assert_eq!(img.width(), w, "width");
        assert_eq!(img.height(), h, "height");
    }

    // --- Stretch ---

    #[test]
    fn stretch_downscales_to_target_dims() {
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(50, 25, FitMode::Stretch, Background::White),
        )
        .expect("stretch downscale");
        assert_round_trip(&out, ImageFormat::Png, 50, 25);
    }

    #[test]
    fn stretch_upscales_to_target_dims() {
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(400, 200, FitMode::Stretch, Background::White),
        )
        .expect("stretch upscale");
        assert_round_trip(&out, ImageFormat::Png, 400, 200);
    }

    #[test]
    fn stretch_produces_target_dims_even_at_different_aspect() {
        // 100×50 source → 200×200 target. Stretch is allowed to distort.
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(200, 200, FitMode::Stretch, Background::White),
        )
        .expect("stretch aspect-mismatched");
        assert_round_trip(&out, ImageFormat::Png, 200, 200);
    }

    // --- Contain: dims + background sampling ---

    #[test]
    fn contain_produces_target_dims_with_letterbox() {
        // 100×50 source → 200×200 target. Contain scales to 200×100 and
        // letterboxes 50px top and 50px bottom.
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(200, 200, FitMode::Contain, Background::White),
        )
        .expect("contain");
        assert_round_trip(&out, ImageFormat::Png, 200, 200);
    }

    #[test]
    fn contain_white_corner_is_white() {
        // 100×50 → 200×200 contain white → top-left pixel sits in the
        // top letterbox (red content is at y∈[50, 150)).
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(200, 200, FitMode::Contain, Background::White),
        )
        .expect("contain white");
        let img = decode_to_rgba(&out);
        let p = img.get_pixel(0, 0);
        assert_eq!(p.0, [255, 255, 255, 255], "top-left corner should be white");
    }

    #[test]
    fn contain_black_corner_is_black() {
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(200, 200, FitMode::Contain, Background::Black),
        )
        .expect("contain black");
        let img = decode_to_rgba(&out);
        let p = img.get_pixel(0, 0);
        assert_eq!(p.0, [0, 0, 0, 255], "top-left corner should be black");
    }

    #[test]
    fn contain_transparent_corner_is_transparent() {
        // PNG supports alpha, so Transparent is allowed.
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(200, 200, FitMode::Contain, Background::Transparent),
        )
        .expect("contain transparent");
        let img = decode_to_rgba(&out);
        let p = img.get_pixel(0, 0);
        assert_eq!(
            p.0[3], 0,
            "top-left corner should be fully transparent, got alpha={}",
            p.0[3]
        );
    }

    #[test]
    fn contain_transparent_on_jpeg_is_rejected() {
        let result = resize_one(
            "jpg",
            &images_fixture("blue.jpg"),
            &opts(200, 200, FitMode::Contain, Background::Transparent),
        );
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(detail.contains("does not support alpha"), "got: {detail}");
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    #[test]
    fn contain_transparent_on_bmp_is_rejected() {
        let result = resize_one(
            "bmp",
            &format_fixture("tiny.bmp"),
            &opts(40, 40, FitMode::Contain, Background::Transparent),
        );
        assert!(matches!(result, Err(AppError::UnsupportedFormat { .. })));
    }

    #[test]
    fn transparent_background_on_stretch_is_ignored_even_for_jpeg() {
        // Stretch never synthesizes background pixels, so a stale
        // `Transparent` choice from the UI is silently ignored — matching
        // the front-end's hide-background-outside-Contain rule.
        let out = resize_one(
            "jpg",
            &images_fixture("blue.jpg"),
            &opts(50, 25, FitMode::Stretch, Background::Transparent),
        )
        .expect("stretch jpeg with stale transparent bg");
        assert_round_trip(&out, ImageFormat::Jpeg, 50, 25);
    }

    // --- Cover ---

    #[test]
    fn cover_produces_target_dims_via_center_crop() {
        // 100×50 source → 80×80 target. Cover scales by max(80/100, 80/50)
        // = 1.6 → resized to 160×80, then center-crops to 80×80.
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(80, 80, FitMode::Cover, Background::White),
        )
        .expect("cover");
        assert_round_trip(&out, ImageFormat::Png, 80, 80);
        // red.png is a uniform (200, 30, 30) source — every pixel of the
        // cover output should match (Lanczos3 on a uniform field doesn't
        // ring, so the assert can be strict).
        let img = decode_to_rgba(&out);
        let p = img.get_pixel(40, 40);
        assert_eq!(
            p.0,
            [200, 30, 30, 255],
            "center of cover should match the uniform source color"
        );
    }

    #[test]
    fn cover_with_matching_aspect_is_pure_scale() {
        // 100×50 → 200×100 cover, same aspect — should equal a plain scale,
        // no crop.
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(200, 100, FitMode::Cover, Background::White),
        )
        .expect("cover same aspect");
        assert_round_trip(&out, ImageFormat::Png, 200, 100);
    }

    // --- Same-aspect target: all three modes produce same dims ---

    #[test]
    fn matching_aspect_target_all_modes_same_dims() {
        for fit in [FitMode::Stretch, FitMode::Contain, FitMode::Cover] {
            let out = resize_one(
                "png",
                &images_fixture("red.png"),
                &opts(200, 100, fit, Background::White),
            )
            .unwrap_or_else(|err| panic!("{fit:?}: {err:?}"));
            assert_round_trip(&out, ImageFormat::Png, 200, 100);
        }
    }

    // --- Per-format round-trips: each of the 5 raster formats resizes
    //     and re-encodes back to the same format. ---

    #[test]
    fn png_resizes_and_preserves_png() {
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(50, 25, FitMode::Stretch, Background::White),
        )
        .expect("resize png");
        assert_round_trip(&out, ImageFormat::Png, 50, 25);
    }

    #[test]
    fn jpeg_resizes_and_preserves_jpeg() {
        let out = resize_one(
            "jpg",
            &images_fixture("blue.jpg"),
            &opts(50, 25, FitMode::Stretch, Background::White),
        )
        .expect("resize jpg");
        assert_round_trip(&out, ImageFormat::Jpeg, 50, 25);
    }

    #[test]
    fn webp_resizes_and_preserves_webp() {
        let out = resize_one(
            "webp",
            &images_fixture("green.webp"),
            &opts(60, 60, FitMode::Stretch, Background::White),
        )
        .expect("resize webp");
        assert_round_trip(&out, ImageFormat::WebP, 60, 60);
    }

    #[test]
    fn bmp_resizes_and_preserves_bmp() {
        let out = resize_one(
            "bmp",
            &format_fixture("tiny.bmp"),
            &opts(40, 20, FitMode::Stretch, Background::White),
        )
        .expect("resize bmp");
        assert_round_trip(&out, ImageFormat::Bmp, 40, 20);
    }

    #[test]
    fn tiff_resizes_and_preserves_tiff() {
        let out = resize_one(
            "tif",
            &format_fixture("tiny.tif"),
            &opts(8, 8, FitMode::Stretch, Background::White),
        )
        .expect("resize tif");
        assert_round_trip(&out, ImageFormat::Tiff, 8, 8);
    }

    // --- Renamed-file convention: output extension drives the encoder. ---

    #[test]
    fn png_bytes_named_jpg_reencode_as_jpeg() {
        let out = resize_one(
            "jpg",
            &images_fixture("red.png"),
            &opts(50, 25, FitMode::Stretch, Background::White),
        )
        .expect("resize png-as-jpg");
        assert_round_trip(&out, ImageFormat::Jpeg, 50, 25);
    }

    // --- EXIF orientation: resize operates in the post-orientation space. ---

    #[test]
    fn exif_oriented_source_is_resized_in_visual_space() {
        // rotated.jpg is stored 100×50 with orientation 6 →
        // decode_oriented yields 50×100. Stretch to 25×50.
        let out = resize_one(
            "jpg",
            &images_fixture("rotated.jpg"),
            &opts(25, 50, FitMode::Stretch, Background::White),
        )
        .expect("resize oriented");
        assert_round_trip(&out, ImageFormat::Jpeg, 25, 50);
    }

    // --- Multi-frame TIFF rejection (end-to-end). ---

    #[test]
    fn multi_frame_tiff_is_rejected() {
        use crate::image::tiff::multi_ifd_tiff;
        let result = resize_one(
            "tif",
            &multi_ifd_tiff(3),
            &opts(8, 8, FitMode::Stretch, Background::White),
        );
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(detail.contains("multi-frame TIFF"), "got: {detail}");
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    // --- Clamp policy ---

    #[test]
    fn zero_dims_clamp_to_one_pixel() {
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(0, 0, FitMode::Stretch, Background::White),
        )
        .expect("resize zero dims");
        assert_round_trip(&out, ImageFormat::Png, 1, 1);
    }

    #[test]
    fn over_max_dim_clamps_to_max() {
        let out = resize_one(
            "png",
            &images_fixture("red.png"),
            &opts(MAX_DIM + 100, 8, FitMode::Stretch, Background::White),
        )
        .expect("resize over-max");
        assert_round_trip(&out, ImageFormat::Png, MAX_DIM, 8);
    }

    // --- Negative / decode-failure paths ---

    #[test]
    fn unsupported_extension_is_rejected() {
        let result = resize_one(
            "gif",
            &format_fixture("tiny.gif"),
            &opts(40, 40, FitMode::Stretch, Background::White),
        );
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(detail.contains("gif"), "got: {detail}");
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    #[test]
    fn garbage_bytes_yield_an_error() {
        let result = resize_one(
            "png",
            &images_fixture("garbage.bin"),
            &opts(10, 10, FitMode::Stretch, Background::White),
        );
        assert!(
            matches!(
                result,
                Err(AppError::UnsupportedFormat { .. } | AppError::ProcessingFailed { .. })
            ),
            "expected decode error, got {result:?}"
        );
    }
}
