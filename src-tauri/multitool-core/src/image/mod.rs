//! Shared image-decoding helpers + the [`RasterFormat`] single source of
//! truth for the formats the pipeline can both decode AND encode.
//!
//! Lives at the workspace level rather than under any one `tools/<tool>/`
//! module so the helper doesn't grow tool-specific knobs. Tools that need
//! to identify a failing input by path can wrap the returned `AppError`
//! themselves — keeping the helper context-free keeps the signature
//! stable as more tools adopt it.

pub mod raster_format;

pub use raster_format::{RasterFormat, RasterFormatDescriptor};

use std::io::Cursor;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader};

use crate::error::{AppError, AppResult};

/// Decode `bytes` into a `DynamicImage`, applying any EXIF orientation tag the
/// decoder exposes.
///
/// `source_ext` is the source file's extension (lowercased, no leading dot)
/// and is consulted **only** when `ImageReader::with_guessed_format()` comes
/// back inconclusive — TGA in particular has no reliable magic and
/// `with_guessed_format` returns no format for it. Bytes always win: a `.tga`
/// file containing PNG data still gets the right decoder. Pass `""` if the
/// caller has no extension to offer (best-effort: anything that needs the
/// extension fallback will fail with `UnsupportedFormat`).
///
/// Errors carry no caller-supplied context. Wrap at the call site if a path
/// or filename belongs in the message.
pub fn decode_oriented(source_ext: &str, bytes: &[u8]) -> AppResult<DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("decode: {err}"),
        })?;
    if reader.format().is_none() && !source_ext.is_empty() {
        if let Some(fmt) = ImageFormat::from_extension(source_ext) {
            reader.set_format(fmt);
        }
    }
    let mut decoder = reader.into_decoder().map_err(image_to_app_err)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(image_to_app_err)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Map an `image::ImageError` to the right `AppError` variant.
/// `Unsupported` / `Decoding` errors land in `UnsupportedFormat`; anything
/// else (I/O, parameter, encoding) falls through to `ProcessingFailed`.
pub fn image_to_app_err(err: ImageError) -> AppError {
    match err {
        ImageError::Unsupported(_) | ImageError::Decoding(_) => AppError::UnsupportedFormat {
            detail: err.to_string(),
        },
        _ => AppError::ProcessingFailed {
            detail: err.to_string(),
        },
    }
}
