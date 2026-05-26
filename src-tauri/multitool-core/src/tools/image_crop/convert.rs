//! Image crop — single-image pure transform.
//!
//! [`crop_one`] decodes the source (EXIF-oriented, via the shared
//! [`decode_oriented`]), clamps the requested rectangle against the image,
//! crops, and re-encodes **in the source extension's format** — the
//! "preserve the source format" contract. JPEG re-encodes at a fixed
//! [`JPEG_QUALITY`]; the other formats ride through `write_to` unchanged.
//!
//! Coordinates are in the post-orientation pixel space (what the user sees in
//! the preview). The output is written upright with no EXIF orientation tag.
//!
//! Clamping policy (see the working doc / DECISIONS): recoverable rects are
//! silently fixed, unrecoverable ones error. Zero-size dims → forced to 1px;
//! partial overflow → clamped to the image intersection; no intersection →
//! `ProcessingFailed`. The clamp lives on [`CropRect::clamp_to`] so it's a
//! reusable primitive, not buried in the orchestrator.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::image::{decode_oriented, image_to_app_err, RasterFormat};

/// Fixed JPEG quality for crop output. Crop is geometric — a single
/// high-quality default keeps the tool focused; finer control is the Image
/// Format Converter's job.
pub(crate) const JPEG_QUALITY: u8 = 90;

/// User-requested crop rectangle, in source-image pixel coords
/// (post-orientation). `x`/`y` are **signed** so a frame the UI dragged
/// partly off-canvas survives the wire without underflow; the backend
/// clamps via [`CropRect::clamp_to`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct CropRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A validated crop region: all-`u32`, guaranteed non-empty and fully within
/// the image it was clamped against. Output of [`CropRect::clamp_to`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    /// Clamp this rect against an `img_w × img_h` image.
    ///
    /// Returns the in-bounds [`PixelRect`] to crop, or `None` when the rect
    /// doesn't intersect the image at all (caller turns that into an error).
    /// Zero-size dimensions are forced to 1px before intersecting, so a
    /// degenerate frame still yields a 1px crop rather than vanishing.
    ///
    /// All math is in `i64` so a far-off-canvas `x`/`y` plus a large width
    /// can't overflow before the clamp.
    pub fn clamp_to(self, img_w: u32, img_h: u32) -> Option<PixelRect> {
        if img_w == 0 || img_h == 0 {
            return None;
        }
        let w = i64::from(self.width.max(1));
        let h = i64::from(self.height.max(1));
        let x0 = i64::from(self.x);
        let y0 = i64::from(self.y);
        let iw = i64::from(img_w);
        let ih = i64::from(img_h);

        let left = x0.max(0);
        let top = y0.max(0);
        let right = (x0 + w).min(iw);
        let bottom = (y0 + h).min(ih);

        if right <= left || bottom <= top {
            return None;
        }
        Some(PixelRect {
            x: left as u32,
            y: top as u32,
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        })
    }
}

/// Crop a single image's bytes to `rect`, preserving the source format.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot).
/// It picks the **output** encoder — so a file whose bytes are PNG but whose
/// name ends `.jpg` re-encodes as JPEG, matching the "keep the extension"
/// rule. Decoding still sniffs the bytes (`decode_oriented`), so the pixels
/// are read correctly regardless of the extension.
///
/// Errors:
/// - `UnsupportedFormat` if the extension isn't an encodable raster format,
///   or the bytes are a multi-frame TIFF (cropping would drop frames).
/// - `ProcessingFailed` if the rect doesn't intersect the image.
/// - decode/encode failures map through [`image_to_app_err`].
pub fn crop_one(source_ext: &str, input_bytes: &[u8], rect: &CropRect) -> AppResult<Vec<u8>> {
    let target = RasterFormat::from_extension(source_ext).ok_or_else(|| {
        AppError::UnsupportedFormat {
            detail: format!(
                "image crop preserves the source format; '{source_ext}' is not an encodable raster format (png/jpg/webp/bmp/tif)"
            ),
        }
    })?;

    // Reject multi-frame TIFF *before* decode — `image` would silently hand
    // back only the first frame, and we can't preserve a multi-page TIFF.
    if let Some(frames) = tiff_frame_count(input_bytes) {
        if frames > 1 {
            return Err(AppError::UnsupportedFormat {
                detail: "multi-frame TIFF is not supported; only single-frame TIFFs can be cropped while preserving the source format".into(),
            });
        }
    }

    let img = decode_oriented(source_ext, input_bytes)?;

    let pixels =
        rect.clamp_to(img.width(), img.height())
            .ok_or_else(|| AppError::ProcessingFailed {
                detail: "crop rectangle does not intersect the image".into(),
            })?;

    let cropped = img.crop_imm(pixels.x, pixels.y, pixels.width, pixels.height);
    encode_to(&cropped, target)
}

/// Encode a cropped image to `format`. JPEG uses the fixed-quality encoder
/// over an RGB buffer (JPEG has no alpha); every other format rides through
/// `write_to`, which preserves alpha for PNG/WebP/TIFF.
///
/// No alpha-handling knob: output format equals input format, so the decoded
/// image's color type is always compatible with re-encoding to that same
/// format (a JPEG source is already RGB; a BMP source is already alpha-less).
fn encode_to(img: &DynamicImage, format: RasterFormat) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    match format {
        RasterFormat::Jpeg => {
            let rgb = img.to_rgb8();
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY);
            encoder.encode_image(&rgb).map_err(image_to_app_err)?;
        }
        other => {
            img.write_to(&mut Cursor::new(&mut bytes), other.image_format())
                .map_err(image_to_app_err)?;
        }
    }
    Ok(bytes)
}

/// Count top-level images (IFDs) in a **classic** TIFF byte stream.
///
/// Returns `Some(n)` for a parseable classic TIFF, `None` for anything else
/// (non-TIFF magic, BigTIFF, or a truncated/malformed chain) — in which case
/// the caller falls through to the normal decoder rather than guessing.
///
/// Walks the IFD linked list: header → first-IFD offset → each IFD is a
/// `u16` entry count, `count × 12` bytes of entries, then a `u32` offset to
/// the next IFD (`0` terminates). The walk is bounded so a corrupt/looping
/// chain can't spin. Any out-of-bounds read aborts to `None`.
fn tiff_frame_count(bytes: &[u8]) -> Option<u32> {
    let little_endian = match bytes.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let read_u16 = |off: usize| -> Option<u16> {
        let s = bytes.get(off..off + 2)?;
        Some(if little_endian {
            u16::from_le_bytes([s[0], s[1]])
        } else {
            u16::from_be_bytes([s[0], s[1]])
        })
    };
    let read_u32 = |off: usize| -> Option<u32> {
        let s = bytes.get(off..off + 4)?;
        Some(if little_endian {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    };

    // Classic TIFF magic is 42; BigTIFF (43) uses 8-byte offsets we don't
    // parse — bail to None and let the decoder handle it.
    if read_u16(2)? != 42 {
        return None;
    }

    let mut ifd_offset = read_u32(4)? as usize;
    let mut count: u32 = 0;
    for _ in 0..1024 {
        if ifd_offset == 0 {
            break;
        }
        let entries = read_u16(ifd_offset)? as usize;
        count = count.saturating_add(1);
        let next_off_pos = ifd_offset.checked_add(2 + entries.checked_mul(12)?)?;
        ifd_offset = read_u32(next_off_pos)? as usize;
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, ImageReader};
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

    fn rect(x: i32, y: i32, width: u32, height: u32) -> CropRect {
        CropRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Re-decode the cropped output and assert format + dimensions.
    fn assert_round_trip(encoded: &[u8], expected_format: ImageFormat, w: u32, h: u32) {
        let reader = ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .expect("guess format on cropped output");
        assert_eq!(reader.format(), Some(expected_format));
        let img = reader.decode().expect("decode cropped output");
        assert_eq!(img.width(), w, "width");
        assert_eq!(img.height(), h, "height");
    }

    // --- Format round-trips: each of the five raster formats crops and
    //     re-encodes to the same format with the cropped dimensions. ---

    #[test]
    fn png_crops_and_preserves_png() {
        let out =
            crop_one("png", &images_fixture("red.png"), &rect(10, 5, 40, 20)).expect("crop png");
        assert_round_trip(&out, ImageFormat::Png, 40, 20);
    }

    #[test]
    fn jpeg_crops_and_preserves_jpeg() {
        let out =
            crop_one("jpg", &images_fixture("blue.jpg"), &rect(0, 0, 50, 25)).expect("crop jpg");
        assert_round_trip(&out, ImageFormat::Jpeg, 50, 25);
    }

    #[test]
    fn webp_crops_and_preserves_webp() {
        let out = crop_one("webp", &images_fixture("green.webp"), &rect(20, 10, 30, 30))
            .expect("crop webp");
        assert_round_trip(&out, ImageFormat::WebP, 30, 30);
    }

    #[test]
    fn bmp_crops_and_preserves_bmp() {
        let out =
            crop_one("bmp", &format_fixture("tiny.bmp"), &rect(5, 5, 20, 10)).expect("crop bmp");
        assert_round_trip(&out, ImageFormat::Bmp, 20, 10);
    }

    #[test]
    fn tiff_crops_and_preserves_tiff() {
        let out =
            crop_one("tif", &format_fixture("tiny.tif"), &rect(0, 0, 16, 16)).expect("crop tif");
        assert_round_trip(&out, ImageFormat::Tiff, 16, 16);
    }

    // --- Renamed-file convention: output extension drives the encoder. ---

    #[test]
    fn png_bytes_named_jpg_reencode_as_jpeg() {
        // red.png is PNG bytes; with a `.jpg` source extension the output
        // must be JPEG (keep the extension, re-encode to match).
        let out = crop_one("jpg", &images_fixture("red.png"), &rect(0, 0, 100, 50))
            .expect("crop png-as-jpg");
        assert_round_trip(&out, ImageFormat::Jpeg, 100, 50);
    }

    // --- EXIF orientation: crop operates in the post-orientation space. ---

    #[test]
    fn exif_oriented_source_is_cropped_in_visual_space() {
        // rotated.jpg is stored 100×50 with orientation 6 → decode_oriented
        // yields a 50×100 image. A full-frame crop must therefore be 50×100,
        // and a sub-crop is taken from the upright image.
        let full = crop_one("jpg", &images_fixture("rotated.jpg"), &rect(0, 0, 50, 100))
            .expect("crop oriented full");
        assert_round_trip(&full, ImageFormat::Jpeg, 50, 100);

        let sub = crop_one("jpg", &images_fixture("rotated.jpg"), &rect(0, 0, 25, 40))
            .expect("crop oriented sub");
        assert_round_trip(&sub, ImageFormat::Jpeg, 25, 40);
    }

    // --- Clamp policy via crop_one (end-to-end). ---

    #[test]
    fn zero_size_rect_is_forced_to_one_pixel() {
        let out = crop_one("png", &images_fixture("red.png"), &rect(10, 10, 0, 0))
            .expect("crop zero-size");
        assert_round_trip(&out, ImageFormat::Png, 1, 1);
    }

    #[test]
    fn partially_out_of_bounds_rect_is_clamped_to_intersection() {
        // red.png is 100×50. x=-5 width=40 → left clamps to 0, right=35.
        // y=40 height=30 → bottom clamps to 50, so height=10.
        let out = crop_one("png", &images_fixture("red.png"), &rect(-5, 40, 40, 30))
            .expect("crop partial-oob");
        assert_round_trip(&out, ImageFormat::Png, 35, 10);
    }

    #[test]
    fn rect_with_no_intersection_errors() {
        // x=100 on a 100-wide image → no columns inside.
        let result = crop_one("png", &images_fixture("red.png"), &rect(100, 0, 10, 10));
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(detail.contains("does not intersect"), "got: {detail}");
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }

    // --- Multi-frame TIFF rejection. ---

    /// Minimal classic little-endian TIFF header with `frames` empty IFDs
    /// chained together. Enough for `tiff_frame_count` to walk the chain;
    /// not a decodable image (rejection happens before decode).
    fn multi_ifd_tiff(frames: usize) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II"); // little-endian
        b.extend_from_slice(&42u16.to_le_bytes()); // classic magic
                                                   // First IFD starts right after the 8-byte header.
        b.extend_from_slice(&8u32.to_le_bytes());
        for i in 0..frames {
            // Each IFD: count=0 entries, then a 4-byte next-IFD offset.
            b.extend_from_slice(&0u16.to_le_bytes());
            let is_last = i + 1 == frames;
            let next = if is_last {
                0u32
            } else {
                // Next IFD sits immediately after this 6-byte IFD.
                u32::try_from(b.len() + 4).unwrap()
            };
            b.extend_from_slice(&next.to_le_bytes());
        }
        b
    }

    #[test]
    fn tiff_frame_count_counts_chained_ifds() {
        assert_eq!(tiff_frame_count(&multi_ifd_tiff(1)), Some(1));
        assert_eq!(tiff_frame_count(&multi_ifd_tiff(2)), Some(2));
        assert_eq!(tiff_frame_count(&multi_ifd_tiff(4)), Some(4));
    }

    #[test]
    fn tiff_frame_count_is_none_for_non_tiff() {
        assert_eq!(tiff_frame_count(&images_fixture("red.png")), None);
        assert_eq!(tiff_frame_count(&images_fixture("blue.jpg")), None);
        assert_eq!(tiff_frame_count(b""), None);
        assert_eq!(tiff_frame_count(b"II"), None); // truncated
    }

    #[test]
    fn single_frame_real_tiff_counts_one() {
        assert_eq!(tiff_frame_count(&format_fixture("tiny.tif")), Some(1));
    }

    #[test]
    fn multi_frame_tiff_is_rejected() {
        let result = crop_one("tif", &multi_ifd_tiff(3), &rect(0, 0, 10, 10));
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(detail.contains("multi-frame TIFF"), "got: {detail}");
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    // --- Unsupported extension + bad bytes. ---

    #[test]
    fn unsupported_output_extension_is_rejected() {
        // GIF can be decoded by the converter, but it's not an encodable
        // raster format, so crop can't preserve it.
        let result = crop_one("gif", &format_fixture("tiny.gif"), &rect(0, 0, 4, 4));
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(detail.contains("gif"), "got: {detail}");
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    #[test]
    fn garbage_bytes_yield_an_error() {
        let result = crop_one("png", &images_fixture("garbage.bin"), &rect(0, 0, 10, 10));
        assert!(
            matches!(
                result,
                Err(AppError::UnsupportedFormat { .. } | AppError::ProcessingFailed { .. })
            ),
            "expected decode error, got {result:?}"
        );
    }

    // --- clamp_to unit matrix (direct). ---

    #[test]
    fn clamp_fully_inside_is_unchanged() {
        assert_eq!(
            rect(10, 10, 30, 20).clamp_to(100, 50),
            Some(PixelRect {
                x: 10,
                y: 10,
                width: 30,
                height: 20
            })
        );
    }

    #[test]
    fn clamp_zero_dims_forced_to_one() {
        assert_eq!(
            rect(5, 5, 0, 0).clamp_to(100, 50),
            Some(PixelRect {
                x: 5,
                y: 5,
                width: 1,
                height: 1
            })
        );
    }

    #[test]
    fn clamp_negative_origin_clamps_to_zero() {
        assert_eq!(
            rect(-5, -5, 20, 20).clamp_to(100, 50),
            Some(PixelRect {
                x: 0,
                y: 0,
                width: 15,
                height: 15
            })
        );
    }

    #[test]
    fn clamp_overflowing_extent_clamps_to_image_edge() {
        assert_eq!(
            rect(90, 40, 50, 50).clamp_to(100, 50),
            Some(PixelRect {
                x: 90,
                y: 40,
                width: 10,
                height: 10
            })
        );
    }

    #[test]
    fn clamp_no_intersection_is_none() {
        assert_eq!(rect(100, 0, 10, 10).clamp_to(100, 50), None); // right of image
        assert_eq!(rect(0, 50, 10, 10).clamp_to(100, 50), None); // below image
        assert_eq!(rect(-10, 0, 10, 10).clamp_to(100, 50), None); // fully left
    }

    #[test]
    fn clamp_against_empty_image_is_none() {
        assert_eq!(rect(0, 0, 10, 10).clamp_to(0, 0), None);
    }
}
