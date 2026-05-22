//! Orchestrates an Images → PDF job end-to-end from a list of input paths.
//!
//! [`run_job`] reads each picked image off disk, threads the bytes into
//! [`super::convert::convert`], and writes the assembled PDF to
//! `unique_path({first_image_parent}/{first_image_stem}.pdf)` per the
//! brief's "first image wins" output rule (`docs/tools/images-to-pdf.md` →
//! Output).
//!
//! **No partial PDF ever exists on cancel.** Unlike PDF → Images (which
//! leaves already-written pages on disk), this orchestrator only writes the
//! output file after [`convert`] returns the complete PDF bytes. If convert
//! returns [`AppError::Cancelled`] (or any other error) the write never
//! happens — the partial-cleanup requirement is satisfied by construction,
//! not by an explicit delete.
//!
//! Progress is surfaced via the `on_progress` callback: the Tauri shell wraps
//! that in `app.emit("tool:progress", …)`; tests call it directly to verify
//! ordering and counts without spinning up Tauri.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{convert, Opts};
use crate::error::{AppError, AppResult};
use crate::fs::unique_path;

/// Progress event payload — `image` is 1-based to match the UX copy
/// ("image 3 / 7" from `docs/tools/images-to-pdf.md` → Running).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub image: u32,
    pub total: u32,
}

/// Final result returned to the caller (and serialized to the webview).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JobResult {
    /// Actual on-disk output path after `unique_path` resolution.
    pub output_path: PathBuf,
    pub page_count: u32,
    pub duration_ms: u64,
}

/// Run a full Images → PDF job: read each input file, assemble the PDF in
/// memory, and write the result to disk at `{first_image_dir}/{first_stem}.pdf`.
///
/// `cancel` is checked before each input read (so a user-cancel during the
/// pre-convert read loop returns quickly) and again by [`convert`] between
/// images. On any non-success path the orchestrator does not write anything,
/// so the brief's "Cancel mid-job deletes the partial PDF" rule holds by
/// construction.
///
/// Errors:
/// - empty `images` slice → `AppError::ProcessingFailed`
/// - input missing → `AppError::FileNotFound`
/// - input unreadable due to permissions → `AppError::PermissionDenied`
/// - other I/O on read or write → `AppError::ProcessingFailed`
/// - convert errors ([`AppError::UnsupportedFormat`], …) bubble unchanged
/// - `on_progress` returning `Err` halts the job and propagates
pub fn run_job<F>(
    images: &[PathBuf],
    opts: &Opts,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();
    if images.is_empty() {
        return Err(AppError::ProcessingFailed {
            detail: "no images to convert".into(),
        });
    }

    let mut buffered: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(images.len());
    for path in images {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let bytes = std::fs::read(path).map_err(|err| io_to_app_err(path, err))?;
        buffered.push((path.clone(), bytes));
    }

    let (pdf_bytes, summary) = convert(
        &buffered,
        opts,
        |p| {
            on_progress(Progress {
                image: p.index.saturating_add(1),
                total: p.total,
            })
        },
        cancel,
    )?;

    let target = derive_output_path(&images[0]);
    let final_path = unique_path(&target).map_err(|err| io_to_app_err(&target, err))?;
    std::fs::write(&final_path, &pdf_bytes).map_err(|err| io_to_app_err(&final_path, err))?;

    Ok(JobResult {
        output_path: final_path,
        page_count: summary.page_count,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// `{first_image_parent}/{first_image_stem}.pdf` — pure path math.
fn derive_output_path(first: &Path) -> PathBuf {
    let mut name: OsString = first.file_stem().map(ToOwned::to_owned).unwrap_or_default();
    name.push(".pdf");
    first
        .parent()
        .map(|p| p.join(&name))
        .unwrap_or_else(|| PathBuf::from(&name))
}

fn io_to_app_err(path: &Path, err: std::io::Error) -> AppError {
    match err.kind() {
        std::io::ErrorKind::NotFound => AppError::FileNotFound {
            path: path.display().to_string(),
        },
        std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied {
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
    use crate::tools::images_to_pdf::PageSize;
    use std::cell::RefCell;
    use std::fs;

    fn fixture_bytes(name: &str) -> Vec<u8> {
        fs::read(format!("tests/fixtures/images/{name}"))
            .unwrap_or_else(|_| panic!("read fixture {name}"))
    }

    /// Stage a fixture under `dir/<rename>` so tests can control the input
    /// path (parent dir, stem) without polluting the checked-in fixtures.
    fn stage(dir: &Path, fixture_name: &str, rename: &str) -> PathBuf {
        let path = dir.join(rename);
        fs::write(&path, fixture_bytes(fixture_name)).unwrap();
        path
    }

    fn opts(page_size: PageSize) -> Opts {
        Opts { page_size }
    }

    #[test]
    fn happy_path_writes_pdf_to_first_image_dir_named_after_first_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let first = stage(tmp.path(), "red.png", "report.png");
        let second = stage(tmp.path(), "blue.jpg", "second.jpg");
        let cancel = CancellationToken::new();
        let progress = RefCell::new(Vec::new());

        let result = run_job(&[first, second], &opts(PageSize::AutoFit), &cancel, |p| {
            progress.borrow_mut().push(p);
            Ok(())
        })
        .unwrap();

        assert_eq!(result.page_count, 2);
        assert_eq!(result.output_path, tmp.path().join("report.pdf"));
        let on_disk = fs::read(&result.output_path).unwrap();
        assert!(on_disk.starts_with(b"%PDF-"), "wrote a non-PDF file?");
        assert_eq!(
            progress.into_inner(),
            vec![
                Progress { image: 1, total: 2 },
                Progress { image: 2, total: 2 },
            ],
        );
    }

    #[test]
    fn first_image_directory_wins_even_when_inputs_span_folders() {
        // Per docs/tools/images-to-pdf.md → Output: the *first* image's
        // folder determines the output location, not a common ancestor.
        let tmp = tempfile::tempdir().unwrap();
        let dir_a = tmp.path().join("alpha");
        let dir_b = tmp.path().join("beta");
        fs::create_dir(&dir_a).unwrap();
        fs::create_dir(&dir_b).unwrap();
        let first = stage(&dir_a, "red.png", "from_a.png");
        let second = stage(&dir_b, "blue.jpg", "from_b.jpg");
        let cancel = CancellationToken::new();

        let result = run_job(&[first, second], &opts(PageSize::AutoFit), &cancel, |_| {
            Ok(())
        })
        .unwrap();

        assert_eq!(result.output_path, dir_a.join("from_a.pdf"));
        assert!(result.output_path.is_file());
    }

    #[test]
    fn missing_input_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let bogus = tmp.path().join("does_not_exist.png");
        let cancel = CancellationToken::new();

        let result = run_job(&[bogus], &opts(PageSize::AutoFit), &cancel, |_| Ok(()));

        match result {
            Err(AppError::FileNotFound { path }) => {
                assert!(path.contains("does_not_exist.png"), "path was {path}");
            }
            other => panic!("expected FileNotFound, got {other:?}"),
        }
    }

    #[test]
    fn cancel_before_read_yields_cancelled_with_no_output_file() {
        let tmp = tempfile::tempdir().unwrap();
        let first = stage(tmp.path(), "red.png", "doc.png");
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = run_job(&[first], &opts(PageSize::AutoFit), &cancel, |_| Ok(()));

        assert!(matches!(result, Err(AppError::Cancelled)), "got {result:?}");
        assert!(
            !tmp.path().join("doc.pdf").exists(),
            "Cancelled job must not leave a partial PDF",
        );
    }

    #[test]
    fn cancel_during_convert_leaves_no_output_file() {
        // First image succeeds, convert checks cancel before the second
        // image, run_job propagates Cancelled. Since the orchestrator only
        // writes after convert succeeds in full, no PDF is ever written —
        // the "partial-cleanup on cancel" requirement is satisfied by
        // construction, not by an explicit delete.
        let tmp = tempfile::tempdir().unwrap();
        let first = stage(tmp.path(), "red.png", "doc.png");
        let second = stage(tmp.path(), "blue.jpg", "next.jpg");
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        let calls = RefCell::new(0u32);

        let result = run_job(&[first, second], &opts(PageSize::AutoFit), &cancel, |_p| {
            *calls.borrow_mut() += 1;
            if *calls.borrow() == 1 {
                signal.cancel();
            }
            Ok(())
        });

        assert!(matches!(result, Err(AppError::Cancelled)), "got {result:?}");
        assert!(
            !tmp.path().join("doc.pdf").exists(),
            "Cancelled job must not leave a partial PDF",
        );
    }

    #[test]
    fn collision_routes_through_unique_path_without_touching_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let first = stage(tmp.path(), "red.png", "doc.png");
        let existing = tmp.path().join("doc.pdf");
        fs::write(&existing, b"keep me").unwrap();
        let cancel = CancellationToken::new();

        let result = run_job(&[first], &opts(PageSize::AutoFit), &cancel, |_| Ok(())).unwrap();

        assert_eq!(result.output_path, tmp.path().join("doc (1).pdf"));
        assert!(result.output_path.is_file());
        assert_eq!(
            fs::read(&existing).unwrap(),
            b"keep me",
            "existing doc.pdf must be untouched",
        );
    }

    #[test]
    fn on_progress_error_halts_and_propagates_without_writing_output() {
        let tmp = tempfile::tempdir().unwrap();
        let first = stage(tmp.path(), "red.png", "doc.png");
        let cancel = CancellationToken::new();

        let result = run_job(&[first], &opts(PageSize::AutoFit), &cancel, |_| {
            Err(AppError::PermissionDenied {
                path: "/tmp/blocked".into(),
            })
        });

        assert!(matches!(result, Err(AppError::PermissionDenied { .. })));
        assert!(
            !tmp.path().join("doc.pdf").exists(),
            "errored job must not leave a partial PDF",
        );
    }

    #[test]
    fn unsupported_input_bytes_propagate_unsupported_format_from_convert() {
        // Typed-error propagation: convert maps garbage bytes to
        // UnsupportedFormat and the orchestrator passes it through
        // unchanged. (We re-verify here so the orchestrator's mapping path
        // is exercised — its only error transform is io_to_app_err, which
        // shouldn't fire for valid-on-disk-but-undecodable inputs.)
        let tmp = tempfile::tempdir().unwrap();
        let bad = stage(tmp.path(), "garbage.bin", "broken.png");
        let cancel = CancellationToken::new();

        let result = run_job(&[bad], &opts(PageSize::AutoFit), &cancel, |_| Ok(()));

        // `decode_oriented` is context-free after the Phase F extraction,
        // so the failing path lives in the surrounding Progress event /
        // orchestrator error envelope rather than in `detail`. The test
        // here just asserts the variant fires.
        assert!(
            matches!(result, Err(AppError::UnsupportedFormat { .. })),
            "expected UnsupportedFormat, got {result:?}",
        );
        assert!(
            !tmp.path().join("broken.pdf").exists(),
            "errored job must not leave output",
        );
    }

    #[test]
    fn empty_input_slice_yields_processing_failed() {
        let cancel = CancellationToken::new();
        let result = run_job(&[], &opts(PageSize::AutoFit), &cancel, |_| Ok(()));
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert_eq!(detail, "no images to convert");
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }

    #[test]
    fn derive_output_path_uses_first_image_stem_and_parent() {
        assert_eq!(
            derive_output_path(Path::new("/tmp/photos/IMG_1234.jpg")),
            PathBuf::from("/tmp/photos/IMG_1234.pdf"),
        );
        assert_eq!(
            derive_output_path(Path::new("local.png")),
            PathBuf::from("local.pdf"),
        );
        assert_eq!(
            derive_output_path(Path::new("./sub/cover.WEBP")),
            PathBuf::from("./sub/cover.pdf"),
        );
    }
}
