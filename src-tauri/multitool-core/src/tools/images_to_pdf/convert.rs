//! Images → PDF pure conversion fn.
//!
//! Takes a slice of `(path, encoded-bytes)` and assembles them into a single
//! PDF with one image per page. The bytes-back-to-the-caller signature
//! (`Result<(Vec<u8>, JobSummary), AppError>`) keeps file I/O in the
//! orchestrator (`job.rs`), which makes the cancel-mid-job rule trivial: if
//! `convert` errors with `Cancelled` the orchestrator never writes anything,
//! so no half-PDF is ever on disk.
//!
//! EXIF orientation is honored on input: `ImageReader::with_guessed_format` →
//! `into_decoder` → `orientation()` → `apply_orientation()`. Without this,
//! phone JPEGs with orientation 6 ("rotate 90 CW") would emerge sideways.
//!
//! Page sizing:
//! - `AutoFit` — each page exactly the post-orientation image dims at 72 DPI.
//! - `A4` / `Letter` — the image is scaled to fit (aspect preserved) and
//!   centered on the standard page.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use printpdf::{ColorBits, ColorSpace, Image, ImageTransform, ImageXObject, Mm, PdfDocument, Px};
use tokio_util::sync::CancellationToken;

use crate::error::AppError;
use crate::image::decode_oriented;

/// Page-size policy for the assembled PDF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageSize {
    /// Each page sized to its image at 72 DPI.
    AutoFit,
    /// 210 × 297 mm; image scale-to-fit + centered.
    A4,
    /// 215.9 × 279.4 mm; image scale-to-fit + centered.
    Letter,
}

/// User-facing options. Mirrors the form fields in the tool view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Opts {
    pub page_size: PageSize,
}

/// Per-image progress event delivered to the `on_page` callback.
///
/// `index` is 0-based in source order; `total` is the source list's length
/// (constant across a job).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageProgress {
    pub index: u32,
    pub total: u32,
}

/// Bookkeeping returned alongside the PDF bytes on successful completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobSummary {
    pub page_count: u32,
    pub duration: Duration,
}

const A4_W_MM: f32 = 210.0;
const A4_H_MM: f32 = 297.0;
const LETTER_W_MM: f32 = 215.9;
const LETTER_H_MM: f32 = 279.4;
const MM_PER_INCH: f32 = 25.4;
const BASE_DPI: f32 = 72.0;

/// Assemble `images` into a single PDF, calling `on_page` after each image is
/// placed on its page.
///
/// Returns early if:
/// - `images` is empty (yields `AppError::ProcessingFailed`)
/// - `cancel` is signalled (yields `AppError::Cancelled`; the most recent
///   `on_page` invocation, if any, still completed)
/// - `on_page` returns an `Err` (the error is propagated; subsequent images
///   are not processed)
/// - an image fails to decode (`UnsupportedFormat` for bad bytes / unknown
///   format, `ProcessingFailed` for other failures)
pub fn convert<F>(
    images: &[(PathBuf, Vec<u8>)],
    opts: &Opts,
    mut on_page: F,
    cancel: &CancellationToken,
) -> Result<(Vec<u8>, JobSummary), AppError>
where
    F: FnMut(PageProgress) -> Result<(), AppError>,
{
    let start = Instant::now();
    let total = u32::try_from(images.len()).unwrap_or(u32::MAX);
    if total == 0 {
        return Err(AppError::ProcessingFailed {
            detail: "no images to convert".into(),
        });
    }

    let doc = PdfDocument::empty("");

    for (index, (path, bytes)) in images.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        let img = decode_oriented(source_ext_of(path), bytes)?;
        let rgb = img.into_rgb8();
        let (img_w_px, img_h_px) = rgb.dimensions();
        let pixel_bytes = rgb.into_raw();

        let (page_w_mm, page_h_mm) = page_size_for(opts.page_size, img_w_px, img_h_px);
        let transform = layout_image(opts.page_size, page_w_mm, page_h_mm, img_w_px, img_h_px);

        let xobject = ImageXObject {
            width: Px(img_w_px as usize),
            height: Px(img_h_px as usize),
            color_space: ColorSpace::Rgb,
            bits_per_component: ColorBits::Bit8,
            image_data: pixel_bytes,
            interpolate: false,
            image_filter: None,
            smask: None,
            clipping_bbox: None,
        };
        let image = Image::from(xobject);

        let (page_idx, layer_idx) = doc.add_page(Mm(page_w_mm), Mm(page_h_mm), "Layer 1");
        let layer = doc.get_page(page_idx).get_layer(layer_idx);
        image.add_to_layer(layer, transform);

        on_page(PageProgress {
            index: index as u32,
            total,
        })?;
    }

    let pdf_bytes = doc
        .save_to_bytes()
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("PDF save: {err}"),
        })?;

    Ok((
        pdf_bytes,
        JobSummary {
            page_count: total,
            duration: start.elapsed(),
        },
    ))
}

/// Lowercased extension of `path` (no leading dot, `""` when absent).
/// Used to disambiguate magic-less formats (TGA) in [`decode_oriented`];
/// when bytes-sniffing succeeds the extension is ignored.
fn source_ext_of(path: &std::path::Path) -> &str {
    path.extension().and_then(|s| s.to_str()).unwrap_or("")
}

fn page_size_for(policy: PageSize, img_w_px: u32, img_h_px: u32) -> (f32, f32) {
    match policy {
        PageSize::AutoFit => (px_to_mm(img_w_px), px_to_mm(img_h_px)),
        PageSize::A4 => (A4_W_MM, A4_H_MM),
        PageSize::Letter => (LETTER_W_MM, LETTER_H_MM),
    }
}

/// Translation + scale for placing the image on its page.
///
/// `AutoFit`: page = image at 72 DPI, so scale = 1 and translation = 0.
/// `A4` / `Letter`: scale-to-fit (aspect preserved) and center.
fn layout_image(
    policy: PageSize,
    page_w_mm: f32,
    page_h_mm: f32,
    img_w_px: u32,
    img_h_px: u32,
) -> ImageTransform {
    let intrinsic_w_mm = px_to_mm(img_w_px);
    let intrinsic_h_mm = px_to_mm(img_h_px);
    let (scale, translate_x, translate_y) = match policy {
        PageSize::AutoFit => (1.0_f32, 0.0_f32, 0.0_f32),
        PageSize::A4 | PageSize::Letter => {
            let scale = (page_w_mm / intrinsic_w_mm).min(page_h_mm / intrinsic_h_mm);
            let scaled_w = intrinsic_w_mm * scale;
            let scaled_h = intrinsic_h_mm * scale;
            (
                scale,
                (page_w_mm - scaled_w) / 2.0,
                (page_h_mm - scaled_h) / 2.0,
            )
        }
    };
    ImageTransform {
        translate_x: Some(Mm(translate_x)),
        translate_y: Some(Mm(translate_y)),
        rotate: None,
        scale_x: Some(scale),
        scale_y: Some(scale),
        dpi: Some(BASE_DPI),
    }
}

fn px_to_mm(px: u32) -> f32 {
    (px as f32) * MM_PER_INCH / BASE_DPI
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(format!("tests/fixtures/images/{name}"))
    }

    fn fixture(name: &str) -> (PathBuf, Vec<u8>) {
        let path = fixture_path(name);
        let bytes = fs::read(&path).unwrap_or_else(|_| panic!("read fixture {name}"));
        (path, bytes)
    }

    fn opts(page_size: PageSize) -> Opts {
        Opts { page_size }
    }

    /// Find `needle` in `haystack` as raw bytes (the PDF embeds binary image
    /// data, so we can't lean on `&str` searches).
    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// Pull the first `/MediaBox[...]` value out of a printpdf-produced PDF.
    /// printpdf 0.7 writes it as `/MediaBox[0 0 W H]` (no space before `[`)
    /// uncompressed in the page-object dictionary; tests using this only
    /// care about the first page.
    fn first_mediabox(pdf: &[u8]) -> (f32, f32) {
        let needle = b"/MediaBox[";
        let start = find_bytes(pdf, needle).expect("MediaBox marker present");
        let rest = &pdf[start + needle.len()..];
        let end = rest
            .iter()
            .position(|b| *b == b']')
            .expect("MediaBox terminator present");
        let inside = std::str::from_utf8(&rest[..end]).expect("MediaBox digits ascii");
        let nums: Vec<f32> = inside
            .split_whitespace()
            .map(|s| s.parse().expect("MediaBox numeric"))
            .collect();
        assert_eq!(nums.len(), 4, "MediaBox should have 4 numbers");
        (nums[2], nums[3])
    }

    /// `/Count N` in the Pages object — total page count of the PDF.
    fn page_count(pdf: &[u8]) -> u32 {
        let needle = b"/Count ";
        let start = find_bytes(pdf, needle).expect("page Count marker present");
        let rest = &pdf[start + needle.len()..];
        let end = rest
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(rest.len());
        let digits = std::str::from_utf8(&rest[..end]).expect("Count digits ascii");
        digits.parse().expect("page Count numeric")
    }

    #[test]
    fn three_images_assemble_into_one_pdf_with_three_pages_and_pdf_magic() {
        let cancel = CancellationToken::new();
        let progress = RefCell::new(Vec::new());
        let images = vec![
            fixture("red.png"),
            fixture("blue.jpg"),
            fixture("green.webp"),
        ];

        let (pdf, summary) = convert(
            &images,
            &opts(PageSize::AutoFit),
            |p| {
                progress.borrow_mut().push(p);
                Ok(())
            },
            &cancel,
        )
        .expect("conversion succeeds");

        assert_eq!(summary.page_count, 3);
        assert!(pdf.starts_with(b"%PDF-"));
        assert_eq!(page_count(&pdf), 3);
        assert_eq!(
            progress.into_inner(),
            vec![
                PageProgress { index: 0, total: 3 },
                PageProgress { index: 1, total: 3 },
                PageProgress { index: 2, total: 3 },
            ],
        );
    }

    #[test]
    fn auto_fit_makes_page_dims_match_image_dims_at_72_dpi() {
        let cancel = CancellationToken::new();
        // red.png is 100×50 (un-rotated). At 72 DPI: page = 100×50 pt.
        let (pdf, _) = convert(
            &[fixture("red.png")],
            &opts(PageSize::AutoFit),
            |_| Ok(()),
            &cancel,
        )
        .expect("conversion succeeds");

        let (w_pt, h_pt) = first_mediabox(&pdf);
        // 100 px → 100/72 inch → 100 pt; allow 1 pt for printpdf's rounding.
        assert!((w_pt - 100.0).abs() < 1.0, "expected ~100pt, got {w_pt}");
        assert!((h_pt - 50.0).abs() < 1.0, "expected ~50pt, got {h_pt}");
    }

    #[test]
    fn a4_page_size_is_used_when_selected() {
        let cancel = CancellationToken::new();
        let (pdf, _) = convert(
            &[fixture("red.png")],
            &opts(PageSize::A4),
            |_| Ok(()),
            &cancel,
        )
        .expect("conversion succeeds");

        let (w_pt, h_pt) = first_mediabox(&pdf);
        // 210mm × 72/25.4 ≈ 595.28 pt; 297mm × 72/25.4 ≈ 841.89 pt.
        assert!((w_pt - 595.28).abs() < 1.5, "A4 width was {w_pt}");
        assert!((h_pt - 841.89).abs() < 1.5, "A4 height was {h_pt}");
    }

    #[test]
    fn letter_page_size_is_used_when_selected() {
        let cancel = CancellationToken::new();
        let (pdf, _) = convert(
            &[fixture("red.png")],
            &opts(PageSize::Letter),
            |_| Ok(()),
            &cancel,
        )
        .expect("conversion succeeds");

        let (w_pt, h_pt) = first_mediabox(&pdf);
        // 215.9mm × 72/25.4 ≈ 612.0 pt; 279.4mm × 72/25.4 ≈ 792.0 pt.
        assert!((w_pt - 612.0).abs() < 1.5, "Letter width was {w_pt}");
        assert!((h_pt - 792.0).abs() < 1.5, "Letter height was {h_pt}");
    }

    #[test]
    fn exif_orientation_six_swaps_image_dimensions() {
        // rotated.jpg is encoded 100×50 (landscape) with EXIF orientation 6
        // ("rotate 90 CW"). After apply_orientation, the image is 50×100
        // (portrait), so the AutoFit page must be portrait too.
        let cancel = CancellationToken::new();
        let (pdf, _) = convert(
            &[fixture("rotated.jpg")],
            &opts(PageSize::AutoFit),
            |_| Ok(()),
            &cancel,
        )
        .expect("conversion succeeds");

        let (w_pt, h_pt) = first_mediabox(&pdf);
        assert!(
            h_pt > w_pt,
            "orientation 6 should yield portrait page; got w={w_pt} h={h_pt}",
        );
        assert!((w_pt - 50.0).abs() < 1.0, "expected ~50pt wide, got {w_pt}");
        assert!(
            (h_pt - 100.0).abs() < 1.0,
            "expected ~100pt tall, got {h_pt}"
        );
    }

    #[test]
    fn unsupported_bytes_yields_unsupported_format() {
        // `decode_oriented` returns context-free errors after the shared-
        // surface extraction. Tools that want the path in the message wrap
        // at the call site; here we just assert the right variant fires.
        let cancel = CancellationToken::new();
        let result = convert(
            &[fixture("garbage.bin")],
            &opts(PageSize::AutoFit),
            |_| Ok(()),
            &cancel,
        );
        assert!(
            matches!(result, Err(AppError::UnsupportedFormat { .. })),
            "expected UnsupportedFormat, got {result:?}",
        );
    }

    #[test]
    fn empty_slice_yields_processing_failed() {
        let cancel = CancellationToken::new();
        let result = convert(&[], &opts(PageSize::AutoFit), |_| Ok(()), &cancel);
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert_eq!(detail, "no images to convert");
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }

    #[test]
    fn cancel_between_images_yields_cancelled_after_partial_progress() {
        let cancel = CancellationToken::new();
        let progress = RefCell::new(Vec::new());
        let images = vec![
            fixture("red.png"),
            fixture("blue.jpg"),
            fixture("green.webp"),
        ];

        let result = convert(
            &images,
            &opts(PageSize::AutoFit),
            |p| {
                progress.borrow_mut().push(p);
                cancel.cancel();
                Ok(())
            },
            &cancel,
        );

        assert!(matches!(result, Err(AppError::Cancelled)), "got {result:?}");
        // First image completed before the cancel checkpoint tripped on
        // iteration 2.
        let progress = progress.into_inner();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].index, 0);
    }

    #[test]
    fn cancel_before_any_image_yields_cancelled_with_no_outputs() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let calls = RefCell::new(0usize);

        let result = convert(
            &[fixture("red.png")],
            &opts(PageSize::AutoFit),
            |_| {
                *calls.borrow_mut() += 1;
                Ok(())
            },
            &cancel,
        );

        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0);
    }

    #[test]
    fn on_page_error_halts_and_propagates() {
        let cancel = CancellationToken::new();
        let calls = RefCell::new(0usize);

        let result = convert(
            &[fixture("red.png"), fixture("blue.jpg")],
            &opts(PageSize::AutoFit),
            |_| {
                *calls.borrow_mut() += 1;
                Err(AppError::PermissionDenied {
                    path: "/tmp/blocked".into(),
                })
            },
            &cancel,
        );

        assert!(matches!(result, Err(AppError::PermissionDenied { .. })));
        assert_eq!(*calls.borrow(), 1, "did not halt after first Err");
    }

    #[test]
    fn webp_input_is_decoded_and_emitted_as_a_pdf_page() {
        // Explicit per-format coverage: the brief calls out .webp as a
        // first-class input, and the `image` crate needs the `webp` feature
        // wired in the workspace Cargo.toml for this to work.
        let cancel = CancellationToken::new();
        let (pdf, summary) = convert(
            &[fixture("green.webp")],
            &opts(PageSize::AutoFit),
            |_| Ok(()),
            &cancel,
        )
        .expect("webp conversion succeeds");

        assert_eq!(summary.page_count, 1);
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn px_to_mm_round_trip_is_consistent_at_72_dpi() {
        // Sanity check on the inverse: 72 px == 1 inch == 25.4 mm.
        assert!((px_to_mm(72) - MM_PER_INCH).abs() < f32::EPSILON);
        assert!((px_to_mm(0) - 0.0).abs() < f32::EPSILON);
    }
}
