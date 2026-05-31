//! Image Compress — single-file pure transform.
//!
//! `(source_ext, bytes, opts)` in, [`CompressedOutput`] out (bytes + the
//! extension the orchestrator should write to). One linear quality slider
//! drives a per-source-format encoder strategy:
//!
//! | Source ext         | Output ext             | Encoder             |
//! | ------------------ | ---------------------- | ------------------- |
//! | `jpg` / `jpeg`     | source spelling kept   | `image` JPEG        |
//! | `webp`             | `webp`                 | `webp` crate, lossy |
//! | `png`/`bmp`/`tif`/`tiff` | `webp`            | `webp` crate, lossy |
//!
//! Lossless inputs (PNG/BMP/TIFF) route to lossy WebP so the slider has
//! meaning — lossy WebP preserves alpha so PNG transparency round-trips;
//! JPEG fallback would silently flatten alpha. See DECISIONS.md → "Image
//! Compress: libwebp via webp crate".
//!
//! EXIF orientation is honored on input (via [`decode_oriented`]) and
//! stripped on output by the decode → re-encode round-trip — same side
//! effect as `image_crop` / `image_resize`.

use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::image::tiff::tiff_frame_count;
use crate::image::{decode_oriented, image_to_app_err};

/// User-facing compress options.
///
/// `quality` is the slider value: `0.0` = smallest file / worst quality,
/// `1.0` = highest quality. Out-of-range values clamp silently — the UI
/// slider is bound to `[0, 1]` anyway, so a stray value over the wire is
/// a defensive case, not an error. Mirrors `video_compress::Opts`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Opts {
    pub quality: f64,
}

/// Output of a single compress: encoded bytes plus the canonical extension
/// the orchestrator should use when writing to disk (no leading dot,
/// lowercased). The extension is part of the return because the per-source
/// strategy can change it (`png` → `webp`) and a JPEG input preserves its
/// source spelling (`jpeg` stays `jpeg`).
#[derive(Clone, Debug)]
pub struct CompressedOutput {
    pub bytes: Vec<u8>,
    pub target_extension: String,
}

/// Linear map from a `0.0..=1.0` slider value to the 1..=100 quality dial
/// both JPEG and lossy WebP accept. Out-of-range values clamp.
///
/// `slider=0.0 → 1`, `slider=1.0 → 100`, `slider=0.5 → 50`,
/// `slider=0.8 → 80` (the default the UI ships with).
pub fn quality_to_q1to100(quality: f64) -> u8 {
    let q = quality.clamp(0.0, 1.0);
    (1.0 + 99.0 * q).round() as u8
}

/// Compress a single image's bytes.
///
/// `source_ext` is the source file's extension (lowercased, no leading
/// dot). The extension — not magic-byte sniffing — drives the per-format
/// strategy table, so a `.jpg` file containing PNG bytes still routes
/// through the JPEG lane (same rule as `image_crop`: bytes win for
/// decode, extension wins for output).
///
/// Errors:
/// - `UnsupportedFormat` if the extension isn't in the compress strategy
///   table, or the bytes are a multi-frame TIFF (re-encoding to a single
///   WebP would silently drop frames).
/// - Decode / encode failures route through [`image_to_app_err`].
pub fn compress_one(
    source_ext: &str,
    input_bytes: &[u8],
    opts: &Opts,
) -> AppResult<CompressedOutput> {
    let lower = source_ext.to_ascii_lowercase();
    let strategy = strategy_for(&lower).ok_or_else(|| AppError::UnsupportedFormat {
        detail: format!(
            "image compress does not support '{source_ext}'; expected one of png/jpg/jpeg/webp/bmp/tif/tiff"
        ),
    })?;

    // Reject multi-frame TIFF before decode — `image`'s TIFF decoder
    // hands back only the first frame, which would silently drop the
    // rest into a single lossy WebP. Same defense the crop + resize
    // tools use.
    if lower == "tif" || lower == "tiff" {
        if let Some(frames) = tiff_frame_count(input_bytes) {
            if frames > 1 {
                return Err(AppError::UnsupportedFormat {
                    detail: "multi-frame TIFF is not supported; only single-frame TIFFs can be compressed".into(),
                });
            }
        }
    }

    let img = decode_oriented(&lower, input_bytes)?;
    let q = quality_to_q1to100(opts.quality);
    let bytes = match strategy.target {
        TargetEncoder::Jpeg => encode_jpeg(&img, q)?,
        TargetEncoder::Webp => encode_webp(&img, q)?,
    };
    Ok(CompressedOutput {
        bytes,
        target_extension: strategy.output_ext.to_string(),
    })
}

/// Which encoder a given source extension routes to, and the extension the
/// output file should carry. `output_ext` is borrowed from the input for
/// JPEG (preserving `.jpg` vs `.jpeg` spelling) so we look it up per call.
struct Strategy {
    target: TargetEncoder,
    output_ext: String,
}

enum TargetEncoder {
    Jpeg,
    Webp,
}

fn strategy_for(source_ext_lower: &str) -> Option<Strategy> {
    match source_ext_lower {
        "jpg" | "jpeg" => Some(Strategy {
            target: TargetEncoder::Jpeg,
            // Preserve the source's spelling so a `.jpeg` doesn't get
            // renamed to `.jpg` mid-batch.
            output_ext: source_ext_lower.to_string(),
        }),
        "webp" | "png" | "bmp" | "tif" | "tiff" => Some(Strategy {
            target: TargetEncoder::Webp,
            output_ext: "webp".to_string(),
        }),
        _ => None,
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> AppResult<Vec<u8>> {
    let rgb = img.to_rgb8();
    let mut bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder.encode_image(&rgb).map_err(image_to_app_err)?;
    Ok(bytes)
}

/// Encode `img` as lossy WebP at `quality` (1..=100), preserving alpha
/// when the source has it. The `webp` crate's `from_image` only accepts
/// RGB8 / RGBA8, so any other color type (Luma, 16-bit, …) is normalized
/// up-front to RGBA8 (lossless conversion for the source pixel data; the
/// lossy WebP encode happens after).
fn encode_webp(img: &DynamicImage, quality: u8) -> AppResult<Vec<u8>> {
    let normalized = match img {
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_) => None,
        // `to_rgba8` is the universal coercion — `webp::Encoder::from_image`
        // would otherwise reject Luma / 16-bit variants with "Unimplemented".
        other => Some(DynamicImage::ImageRgba8(other.to_rgba8())),
    };
    let target = normalized.as_ref().unwrap_or(img);
    let encoder = webp::Encoder::from_image(target).map_err(|err| AppError::ProcessingFailed {
        detail: format!("webp encoder init: {err}"),
    })?;
    let memory = encoder.encode(f32::from(quality));
    Ok(memory.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, ImageReader};
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn images_fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(format!("tests/fixtures/images/{name}"));
        fs::read(&path).unwrap_or_else(|_| panic!("read images fixture {name}: {path:?}"))
    }

    fn format_fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(format!("tests/fixtures/image_format/{name}"));
        fs::read(&path).unwrap_or_else(|_| panic!("read image_format fixture {name}: {path:?}"))
    }

    fn opts(quality: f64) -> Opts {
        Opts { quality }
    }

    fn assert_decodes_as(bytes: &[u8], expected: ImageFormat, w: u32, h: u32) {
        let reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .expect("guess format on encoded output");
        assert_eq!(reader.format(), Some(expected));
        let img = reader.decode().expect("decode encoded output");
        assert_eq!(img.width(), w);
        assert_eq!(img.height(), h);
    }

    // --- Quality mapping ---

    #[test]
    fn quality_zero_maps_to_one() {
        assert_eq!(quality_to_q1to100(0.0), 1);
    }

    #[test]
    fn quality_one_maps_to_one_hundred() {
        assert_eq!(quality_to_q1to100(1.0), 100);
    }

    #[test]
    fn quality_half_maps_to_fifty() {
        // 1 + 99*0.5 = 50.5 → rounds to 51 (banker's vs. half-up: `f64::round`
        // is round-half-away-from-zero, so 50.5 → 51).
        assert_eq!(quality_to_q1to100(0.5), 51);
    }

    #[test]
    fn quality_default_eighty_maps_to_eighty() {
        // 1 + 99*0.8 = 80.2 → rounds to 80.
        assert_eq!(quality_to_q1to100(0.8), 80);
    }

    #[test]
    fn quality_clamps_out_of_range_values() {
        assert_eq!(quality_to_q1to100(-1.0), 1);
        assert_eq!(quality_to_q1to100(5.0), 100);
    }

    // --- Per-source-format dispatch ---

    #[test]
    fn jpg_source_stays_jpeg_with_jpg_extension() {
        let out = compress_one("jpg", &images_fixture("blue.jpg"), &opts(0.8))
            .expect("compress JPEG → JPEG");
        assert_eq!(out.target_extension, "jpg");
        assert_decodes_as(&out.bytes, ImageFormat::Jpeg, 100, 50);
    }

    #[test]
    fn jpeg_source_preserves_jpeg_extension_spelling() {
        // The same JPEG bytes named `.jpeg` should land in a `.jpeg` output,
        // not `.jpg` — preserve the source's spelling.
        let out = compress_one("jpeg", &images_fixture("blue.jpg"), &opts(0.8))
            .expect("compress JPEG (spelling: jpeg) → JPEG");
        assert_eq!(out.target_extension, "jpeg");
        assert_decodes_as(&out.bytes, ImageFormat::Jpeg, 100, 50);
    }

    #[test]
    fn jpeg_source_extension_lowercases() {
        // Picker gives lowercase already but the conversion is defensive —
        // a `.JPG` over the wire should also land at `.jpg`.
        let out = compress_one("JPG", &images_fixture("blue.jpg"), &opts(0.8))
            .expect("compress upper JPG → JPEG");
        assert_eq!(out.target_extension, "jpg");
    }

    #[test]
    fn webp_source_stays_webp() {
        let out = compress_one("webp", &images_fixture("green.webp"), &opts(0.8))
            .expect("compress WebP → WebP");
        assert_eq!(out.target_extension, "webp");
        assert_decodes_as(&out.bytes, ImageFormat::WebP, 100, 50);
    }

    #[test]
    fn png_source_routes_to_webp() {
        let out = compress_one("png", &images_fixture("red.png"), &opts(0.8))
            .expect("compress PNG → WebP");
        assert_eq!(out.target_extension, "webp");
        assert_decodes_as(&out.bytes, ImageFormat::WebP, 100, 50);
    }

    #[test]
    fn bmp_source_routes_to_webp() {
        let out = compress_one("bmp", &format_fixture("tiny.bmp"), &opts(0.8))
            .expect("compress BMP → WebP");
        assert_eq!(out.target_extension, "webp");
        assert_decodes_as(&out.bytes, ImageFormat::WebP, 100, 50);
    }

    #[test]
    fn tif_source_routes_to_webp() {
        let out = compress_one("tif", &format_fixture("tiny.tif"), &opts(0.8))
            .expect("compress TIFF → WebP");
        assert_eq!(out.target_extension, "webp");
        assert_decodes_as(&out.bytes, ImageFormat::WebP, 100, 50);
    }

    #[test]
    fn tiff_extension_spelling_also_routes_to_webp() {
        // The picker accepts both `.tif` and `.tiff`; both must route to WebP.
        let out = compress_one("tiff", &format_fixture("tiny.tif"), &opts(0.8))
            .expect("compress .tiff → WebP");
        assert_eq!(out.target_extension, "webp");
    }

    // --- PNG with alpha → WebP preserves alpha ---

    #[test]
    fn png_with_alpha_round_trips_through_webp_with_alpha_channel() {
        // alpha.png: 4×4, left half opaque red, right half fully transparent.
        // After PNG → lossy WebP the right half should still be transparent.
        let out = compress_one("png", &format_fixture("alpha.png"), &opts(0.8))
            .expect("compress alpha PNG → WebP");
        let decoded = ImageReader::new(Cursor::new(&out.bytes))
            .with_guessed_format()
            .expect("re-read encoded WebP")
            .decode()
            .expect("decode WebP")
            .to_rgba8();
        // Lossy WebP perturbs colors slightly, but alpha at 255 / 0 should
        // round-trip cleanly enough to stay distinguishable.
        let left = decoded.get_pixel(0, 0).0;
        let right = decoded.get_pixel(3, 0).0;
        assert!(
            left[3] > 200,
            "left should stay opaque, got alpha={}",
            left[3]
        );
        assert!(
            right[3] < 50,
            "right should stay transparent, got alpha={}",
            right[3]
        );
    }

    // --- Slider extremes change file size ---

    #[test]
    fn jpeg_high_quality_produces_larger_output_than_low() {
        let lo = compress_one("jpg", &images_fixture("blue.jpg"), &opts(0.05)).expect("lo q jpeg");
        let hi = compress_one("jpg", &images_fixture("blue.jpg"), &opts(0.95)).expect("hi q jpeg");
        assert!(
            hi.bytes.len() > lo.bytes.len(),
            "expected hi-q ({}) > lo-q ({})",
            hi.bytes.len(),
            lo.bytes.len()
        );
    }

    #[test]
    fn webp_high_quality_produces_larger_output_than_low() {
        let lo = compress_one("png", &images_fixture("red.png"), &opts(0.05)).expect("lo q webp");
        let hi = compress_one("png", &images_fixture("red.png"), &opts(0.95)).expect("hi q webp");
        assert!(
            hi.bytes.len() > lo.bytes.len(),
            "expected hi-q ({}) > lo-q ({})",
            hi.bytes.len(),
            lo.bytes.len()
        );
    }

    // --- Extension-wins-for-output: PNG bytes named .jpg still go through
    //     the JPEG lane (matches image_crop's rule). ---

    #[test]
    fn png_bytes_named_jpg_route_through_jpeg_lane() {
        let out = compress_one("jpg", &images_fixture("red.png"), &opts(0.8))
            .expect("compress PNG-bytes-as-jpg → JPEG");
        assert_eq!(out.target_extension, "jpg");
        assert_decodes_as(&out.bytes, ImageFormat::Jpeg, 100, 50);
    }

    // --- EXIF orientation honored on input ---

    #[test]
    fn exif_oriented_source_compresses_in_upright_space() {
        // rotated.jpg is stored 100×50 with orientation 6 → decoded as 50×100.
        // The compressed JPEG output should carry the upright dimensions.
        let out = compress_one("jpg", &images_fixture("rotated.jpg"), &opts(0.8))
            .expect("compress oriented JPEG");
        assert_decodes_as(&out.bytes, ImageFormat::Jpeg, 50, 100);
    }

    // --- Error cases ---

    #[test]
    fn unsupported_extension_yields_unsupported_format() {
        let result = compress_one("gif", b"", &opts(0.8));
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(
                    detail.contains("gif"),
                    "expected 'gif' in detail, got: {detail}"
                );
            }
            other => panic!("expected UnsupportedFormat for .gif, got {other:?}"),
        }
    }

    #[test]
    fn empty_extension_yields_unsupported_format() {
        let result = compress_one("", &images_fixture("red.png"), &opts(0.8));
        assert!(matches!(result, Err(AppError::UnsupportedFormat { .. })));
    }

    #[test]
    fn garbage_bytes_yield_decode_error() {
        let result = compress_one("png", &images_fixture("garbage.bin"), &opts(0.8));
        assert!(
            matches!(
                result,
                Err(AppError::UnsupportedFormat { .. } | AppError::ProcessingFailed { .. })
            ),
            "expected decode error, got {result:?}"
        );
    }

    #[test]
    fn multi_frame_tiff_is_rejected() {
        use crate::image::tiff::multi_ifd_tiff;
        let result = compress_one("tif", &multi_ifd_tiff(3), &opts(0.8));
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(
                    detail.contains("multi-frame TIFF"),
                    "expected multi-frame TIFF detail, got: {detail}"
                );
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }
}
