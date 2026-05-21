//! PDF → Images pure conversion fn.
//!
//! The webview's UI options (`format`, `dpi`) map directly to [`Opts`]. The
//! `convert` fn renders each page to a bitmap at the requested DPI, encodes
//! it as PNG or JPEG, and hands the bytes to the caller via the `on_page`
//! callback (see `DECISIONS.md` → "Streaming `on_page` callback" for the
//! memory-pressure rationale).
//!
//! Cancellation is checked between pages, so a request to stop a 100-page job
//! takes effect within one page's render time. Callers that need finer
//! granularity should slice their work differently.
//!
//! Error mapping is documented inline on [`map_load_error`] /
//! [`map_render_error`]; the only typed UI-visible branch is
//! [`AppError::Encrypted`].

use std::io::Cursor;
use std::time::{Duration, Instant};

use image::ImageFormat;
use pdfium_render::prelude::{PdfRenderConfig, PdfiumError, PdfiumInternalError};
use tokio_util::sync::CancellationToken;

use crate::error::AppError;
use crate::pdfium;

/// Output image format for the rendered pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Png,
    Jpeg,
}

impl Format {
    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
        }
    }
}

/// User-facing options. Mirrors the form fields in the tool view.
///
/// **`dpi` is clamped** to `[DPI_MIN, DPI_MAX]` (72–600) inside `convert` —
/// the UI clamps for UX, this defends the renderer against unbounded values
/// from buggy or hostile callers. Values outside the range are silently moved
/// to the nearest bound; no error is returned. See the DPI-clamp tests below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Opts {
    pub format: Format,
    pub dpi: u32,
}

/// One rendered + encoded page passed to the `on_page` callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageOutput {
    /// 0-based page index in source-document order.
    pub index: u32,
    /// PNG- or JPEG-encoded bytes (per `Opts::format`).
    pub encoded: Vec<u8>,
}

/// Summary returned on successful completion. The streamed `PageOutput`s
/// carry the actual data; this is bookkeeping for the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobSummary {
    pub page_count: u32,
    pub duration: Duration,
}

pub const DPI_MIN: u32 = 72;
pub const DPI_MAX: u32 = 600;

/// Render each page of `pdf_bytes` to an image, streaming one [`PageOutput`]
/// to `on_page` per page in source order.
///
/// Returns early if:
/// - `cancel` is signalled (yields `AppError::Cancelled`; the most recent
///   `on_page` invocation, if any, still completed)
/// - `on_page` returns an `Err` (the error is propagated; subsequent pages
///   are not rendered)
/// - the document is encrypted (yields `AppError::Encrypted`, no `on_page`
///   calls)
/// - the document has zero pages (yields `AppError::ProcessingFailed`)
pub fn convert<F>(
    pdf_bytes: &[u8],
    opts: &Opts,
    mut on_page: F,
    cancel: &CancellationToken,
) -> Result<JobSummary, AppError>
where
    F: FnMut(PageOutput) -> Result<(), AppError>,
{
    let start = Instant::now();

    let pdfium = pdfium::instance()?;
    let document = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, None)
        .map_err(map_load_error)?;

    // pdfium-render returns `i32`; counts can't be negative in practice, so
    // negative values collapse to 0 (caught by the empty-document check).
    let page_count = u32::try_from(document.pages().len()).unwrap_or(0);
    if page_count == 0 {
        return Err(AppError::ProcessingFailed {
            detail: "empty document".into(),
        });
    }

    let dpi = opts.dpi.clamp(DPI_MIN, DPI_MAX);
    let scale = dpi as f32 / 72.0;
    let image_format = opts.format.image_format();
    let config = PdfRenderConfig::new().scale_page_by_factor(scale);

    for (index, page) in document.pages().iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        let bitmap = page.render_with_config(&config).map_err(map_render_error)?;
        let image = bitmap.as_image().map_err(map_render_error)?;

        let mut encoded = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut encoded), image_format)
            .map_err(|err| AppError::ProcessingFailed {
                detail: format!("encode page {index}: {err}"),
            })?;

        on_page(PageOutput {
            index: index as u32,
            encoded,
        })?;
    }

    Ok(JobSummary {
        page_count,
        duration: start.elapsed(),
    })
}

/// Map a `PdfiumError` from `load_pdf_*` to an `AppError`.
///
/// Encrypted / security-locked PDFs collapse onto `AppError::Encrypted` so the
/// UI can show its dedicated "no password entry in Phase 1" message. Every
/// other failure is opaque to the UI and lands under `ProcessingFailed` with
/// the raw debug rendering of the pdfium error in the detail (good enough for
/// a toast; not meant for end-user diagnostic value).
fn map_load_error(err: PdfiumError) -> AppError {
    match err {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)
        | PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::SecurityError) => {
            AppError::Encrypted
        }
        other => AppError::ProcessingFailed {
            detail: format!("{other:?}"),
        },
    }
}

fn map_render_error(err: PdfiumError) -> AppError {
    AppError::ProcessingFailed {
        detail: format!("{err:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs;

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(format!("tests/fixtures/{name}")).unwrap_or_else(|_| panic!("read fixture {name}"))
    }

    fn opts(format: Format, dpi: u32) -> Opts {
        Opts { format, dpi }
    }

    fn collect(bytes: &[u8], opts: &Opts) -> (Vec<PageOutput>, JobSummary) {
        let pages = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let summary = convert(
            bytes,
            opts,
            |page| {
                pages.borrow_mut().push(page);
                Ok(())
            },
            &cancel,
        )
        .expect("conversion succeeds");
        (pages.into_inner(), summary)
    }

    const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF];

    #[test]
    fn three_pages_png_streams_three_outputs_with_png_magic() {
        let (pages, summary) = collect(&fixture("three-page.pdf"), &opts(Format::Png, 72));

        assert_eq!(summary.page_count, 3);
        assert_eq!(pages.len(), 3);
        for (i, page) in pages.iter().enumerate() {
            assert_eq!(page.index, i as u32);
            assert!(
                page.encoded.starts_with(PNG_MAGIC),
                "page {i} not PNG-encoded"
            );
        }
    }

    #[test]
    fn single_page_jpeg_has_jpeg_magic() {
        let (pages, summary) = collect(&fixture("single-page.pdf"), &opts(Format::Jpeg, 72));

        assert_eq!(summary.page_count, 1);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].encoded.starts_with(JPEG_MAGIC));
    }

    #[test]
    fn higher_dpi_produces_larger_decoded_dimensions() {
        let pdf = fixture("single-page.pdf");
        let (low, _) = collect(&pdf, &opts(Format::Png, 72));
        let (high, _) = collect(&pdf, &opts(Format::Png, 300));

        let low_dims = image::load_from_memory(&low[0].encoded)
            .expect("decode low-dpi PNG")
            .into_rgba8()
            .dimensions();
        let high_dims = image::load_from_memory(&high[0].encoded)
            .expect("decode high-dpi PNG")
            .into_rgba8()
            .dimensions();

        // 300 / 72 ≈ 4.17x linear, so both axes must grow well beyond rounding noise.
        assert!(
            high_dims.0 > low_dims.0 * 3 && high_dims.1 > low_dims.1 * 3,
            "expected ~4x growth, got low={low_dims:?} high={high_dims:?}",
        );
    }

    #[test]
    fn encrypted_pdf_maps_to_encrypted_variant() {
        let cancel = CancellationToken::new();
        let result = convert(
            &fixture("encrypted.pdf"),
            &opts(Format::Png, 72),
            |_| Ok(()),
            &cancel,
        );
        assert!(matches!(result, Err(AppError::Encrypted)), "got {result:?}");
    }

    #[test]
    fn corrupt_pdf_maps_to_processing_failed() {
        let cancel = CancellationToken::new();
        let result = convert(
            &fixture("corrupt.pdf"),
            &opts(Format::Png, 72),
            |_| Ok(()),
            &cancel,
        );
        assert!(
            matches!(result, Err(AppError::ProcessingFailed { .. })),
            "got {result:?}",
        );
    }

    #[test]
    fn zero_page_pdf_maps_to_processing_failed_with_empty_detail() {
        let cancel = CancellationToken::new();
        let result = convert(
            &fixture("zero-page.pdf"),
            &opts(Format::Png, 72),
            |_| Ok(()),
            &cancel,
        );
        // Two paths reach this branch: pdfium might reject the zero-page PDF
        // outright with FormatError, or it might load it and our explicit
        // empty check fires. Both surface as ProcessingFailed — that's the
        // contract the plan calls for. If pdfium accepted it, the detail
        // string is exactly "empty document".
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert!(
                    detail == "empty document" || detail.contains("FormatError"),
                    "unexpected ProcessingFailed detail: {detail}",
                );
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }

    #[test]
    fn cancellation_between_pages_yields_cancelled_after_partial_progress() {
        let cancel = CancellationToken::new();
        let pages = RefCell::new(Vec::new());

        let result = convert(
            &fixture("three-page.pdf"),
            &opts(Format::Png, 72),
            |page| {
                pages.borrow_mut().push(page);
                cancel.cancel();
                Ok(())
            },
            &cancel,
        );

        assert!(matches!(result, Err(AppError::Cancelled)), "got {result:?}");
        // Exactly the first page completed before the cancel checkpoint
        // tripped on the next iteration.
        let pages = pages.into_inner();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].index, 0);
    }

    #[test]
    fn cancel_before_any_page_yields_cancelled_with_no_outputs() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let pages = RefCell::new(0usize);
        let result = convert(
            &fixture("three-page.pdf"),
            &opts(Format::Png, 72),
            |_| {
                *pages.borrow_mut() += 1;
                Ok(())
            },
            &cancel,
        );

        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*pages.borrow(), 0);
    }

    #[test]
    fn on_page_error_halts_and_propagates() {
        let cancel = CancellationToken::new();
        let calls = RefCell::new(0usize);

        let result = convert(
            &fixture("three-page.pdf"),
            &opts(Format::Png, 72),
            |_| {
                *calls.borrow_mut() += 1;
                Err(AppError::PermissionDenied {
                    path: "/tmp/blocked".into(),
                })
            },
            &cancel,
        );

        assert!(matches!(result, Err(AppError::PermissionDenied { .. })));
        assert_eq!(
            *calls.borrow(),
            1,
            "conversion did not halt after first Err"
        );
    }

    #[test]
    fn dpi_above_max_is_clamped_silently() {
        let pdf = fixture("single-page.pdf");
        let (at_max, _) = collect(&pdf, &opts(Format::Png, DPI_MAX));
        let (above_max, _) = collect(&pdf, &opts(Format::Png, 9_999));

        let max_dims = image::load_from_memory(&at_max[0].encoded)
            .expect("decode at-max PNG")
            .into_rgba8()
            .dimensions();
        let above_dims = image::load_from_memory(&above_max[0].encoded)
            .expect("decode above-max PNG")
            .into_rgba8()
            .dimensions();

        assert_eq!(
            max_dims, above_dims,
            "dpi above DPI_MAX should clamp, not scale further",
        );
    }

    #[test]
    fn dpi_below_min_is_clamped_silently() {
        let pdf = fixture("single-page.pdf");
        let (at_min, _) = collect(&pdf, &opts(Format::Png, DPI_MIN));
        let (below_min, _) = collect(&pdf, &opts(Format::Png, 0));

        let min_dims = image::load_from_memory(&at_min[0].encoded)
            .expect("decode at-min PNG")
            .into_rgba8()
            .dimensions();
        let below_dims = image::load_from_memory(&below_min[0].encoded)
            .expect("decode below-min PNG")
            .into_rgba8()
            .dimensions();

        assert_eq!(
            min_dims, below_dims,
            "dpi below DPI_MIN should clamp, not shrink further",
        );
    }
}
