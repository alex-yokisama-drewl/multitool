//! Image Crop — single-image orchestrator.
//!
//! Pipeline: read source bytes → cancel check → crop (decode + clamp +
//! re-encode, in [`super::convert::crop_one`]) → write to
//! `{stem}_cropped.{ext}` (or the next free `(n)` suffix via
//! [`crate::fs::unique_path`]).
//!
//! Output format = source format (the crop contract). Cancellation is
//! checked before read and again after read / before the crop work — crop
//! itself is a single CPU-bound transform with no internal checkpoints, the
//! same shape as the other single-file tools.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{crop_one, CropRect};
use crate::error::{AppError, AppResult};
use crate::fs::unique_path;

/// Per-job event streamed to the UI. Single variant: the crop fires
/// `Started` once, then resolves with [`JobResult`] or rejects with an
/// `AppError`. No skip-and-continue path — there's only one image.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to read + crop the picked image.
    Started { source: PathBuf },
}

/// Result of a completed crop.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    /// Where the cropped bytes were written. Routes through `unique_path`,
    /// so a same-name collision lands at `{stem}_cropped (1).{ext}`. Pass
    /// straight to `revealItemInDir`.
    pub output: PathBuf,
    pub duration_ms: u64,
}

/// Run the crop job end-to-end. Returns once the cropped bytes are written,
/// or earlier with an `AppError` if anything fails.
pub fn run_job<F>(
    source: &Path,
    rect: &CropRect,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();

    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    on_progress(Progress::Started {
        source: source.to_path_buf(),
    })?;

    let bytes = fs::read(source).map_err(|err| io_to_app_err(source, &err))?;
    let source_ext = source
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    let cropped = crop_one(&source_ext, &bytes, rect)?;

    let target_path = derive_output_path(source);
    let final_path = unique_path(&target_path).map_err(|err| io_to_app_err(&target_path, &err))?;
    fs::write(&final_path, &cropped).map_err(|err| io_to_app_err(&final_path, &err))?;

    Ok(JobResult {
        output: final_path,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Compute the desired output path, **before** `unique_path` resolution.
///
/// `photo.png` → `photo_cropped.png`, preserving the source directory and
/// extension. A missing extension falls back to a bare `_cropped` suffix —
/// the picker filter should make that unreachable, but the branch keeps the
/// orchestrator honest.
fn derive_output_path(source: &Path) -> PathBuf {
    let parent = source.parent();
    let stem = source.file_stem().map(|s| s.to_owned()).unwrap_or_default();
    let mut name = stem;
    name.push("_cropped");
    if let Some(ext) = source.extension() {
        name.push(".");
        name.push(ext);
    }
    match parent {
        Some(p) => p.join(name),
        None => PathBuf::from(name),
    }
}

fn io_to_app_err(path: &Path, err: &io::Error) -> AppError {
    match err.kind() {
        io::ErrorKind::NotFound => AppError::FileNotFound {
            path: path.display().to_string(),
        },
        io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
            path: path.display().to_string(),
        },
        _ => AppError::ProcessingFailed {
            detail: format!("{}: {err}", path.display()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn images_fixture_path(name: &str) -> PathBuf {
        PathBuf::from(format!("tests/fixtures/images/{name}"))
    }

    fn copy_fixture_to(dir: &Path, name: &str) -> PathBuf {
        let dst = dir.join(name);
        fs::copy(images_fixture_path(name), &dst).expect("copy fixture");
        dst
    }

    fn rect(x: i32, y: i32, width: u32, height: u32) -> CropRect {
        CropRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Minimal classic little-endian multi-IFD TIFF (no pixel data) — enough
    /// for the convert layer's frame counter to reject it. Mirrors the
    /// helper in `convert.rs` tests so the orchestrator can prove it
    /// propagates the rejection + writes nothing.
    fn multi_ifd_tiff(frames: usize) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"II");
        b.extend_from_slice(&42u16.to_le_bytes());
        b.extend_from_slice(&8u32.to_le_bytes());
        for i in 0..frames {
            b.extend_from_slice(&0u16.to_le_bytes());
            let next = if i + 1 == frames {
                0u32
            } else {
                u32::try_from(b.len() + 4).unwrap()
            };
            b.extend_from_slice(&next.to_le_bytes());
        }
        b
    }

    #[test]
    fn happy_path_writes_cropped_png_with_expected_name() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "red.png");

        let cancel = CancellationToken::new();
        let events = RefCell::new(Vec::new());
        let result = run_job(&input, &rect(10, 5, 40, 20), &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("crop ok");

        let expected = dir.path().join("red_cropped.png");
        assert_eq!(result.output, expected);
        assert!(expected.exists(), "expected output at {expected:?}");

        // One Started event carrying the source path.
        assert_eq!(events.borrow().len(), 1);
        match &events.borrow()[0] {
            Progress::Started { source } => assert_eq!(source, &input),
        }

        // Output decodes to the cropped dimensions.
        let bytes = fs::read(&expected).expect("read cropped");
        let img = image::load_from_memory(&bytes).expect("decode cropped png");
        assert_eq!(img.width(), 40);
        assert_eq!(img.height(), 20);
    }

    #[test]
    fn missing_input_file_is_returned_as_file_not_found() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("nope.png");
        let cancel = CancellationToken::new();
        let result = run_job(&missing, &rect(0, 0, 10, 10), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::FileNotFound { .. })));
    }

    #[test]
    fn invalid_rect_propagates_and_writes_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "red.png");
        let cancel = CancellationToken::new();
        // x=100 on a 100-wide image → no intersection.
        let result = run_job(&input, &rect(100, 0, 10, 10), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        assert!(!dir.path().join("red_cropped.png").exists());
    }

    #[test]
    fn multi_frame_tiff_is_rejected_and_writes_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let input = dir.path().join("frames.tif");
        fs::write(&input, multi_ifd_tiff(3)).expect("write multi-frame tiff");
        let cancel = CancellationToken::new();
        let result = run_job(&input, &rect(0, 0, 4, 4), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::UnsupportedFormat { .. })));
        assert!(!dir.path().join("frames_cropped.tif").exists());
    }

    #[test]
    fn cancel_before_read_returns_cancelled_with_no_writes() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "red.png");
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&input, &rect(0, 0, 40, 20), &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0, "no progress events fire pre-cancel");
        assert!(!dir.path().join("red_cropped.png").exists());
    }

    #[test]
    fn cancel_after_started_returns_cancelled_without_writing() {
        // Cancel from inside the Started callback; the post-read cancel
        // checkpoint catches it before the crop / write.
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "red.png");
        let cancel = CancellationToken::new();

        let result = run_job(&input, &rect(0, 0, 40, 20), &cancel, |progress| {
            if matches!(progress, Progress::Started { .. }) {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert!(!dir.path().join("red_cropped.png").exists());
    }

    #[test]
    fn output_name_collision_routes_through_unique_path() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "red.png");
        let placeholder = dir.path().join("red_cropped.png");
        fs::write(&placeholder, b"placeholder").expect("write placeholder");
        let placeholder_bytes = fs::read(&placeholder).expect("read placeholder");

        let cancel = CancellationToken::new();
        let result = run_job(&input, &rect(0, 0, 40, 20), &cancel, |_| Ok(()))
            .expect("crop ok despite collision");

        let new_path = dir.path().join("red_cropped (1).png");
        assert!(new_path.exists(), "expected {new_path:?}");
        assert_eq!(result.output, new_path);
        // Placeholder untouched.
        assert_eq!(fs::read(&placeholder).unwrap(), placeholder_bytes);
    }

    #[test]
    fn on_progress_error_aborts_before_read() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "red.png");
        let cancel = CancellationToken::new();
        let result = run_job(&input, &rect(0, 0, 40, 20), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        assert!(!dir.path().join("red_cropped.png").exists());
    }
}
