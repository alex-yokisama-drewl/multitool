//! Orchestrates a PDF → Images job end-to-end from a file path.
//!
//! [`run_job`] reads the PDF off disk, derives the sibling output folder
//! (`{stem}_pages/`), and threads [`super::convert::convert`]'s streaming
//! `on_page` callback into a [`super::writer::PageWriter`] that's created
//! lazily on the first page — so a doomed convert (encrypted / empty PDF)
//! never leaves an empty folder behind.
//!
//! Progress is surfaced via the `on_progress` callback: the Tauri shell wraps
//! that in `app.emit("tool:progress", …)`; tests call it directly to verify
//! ordering and counts without spinning up Tauri. Same shape as
//! `convert`'s `on_page`: streaming + cancellable + error-propagating.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{convert, Opts};
use super::writer::PageWriter;
use crate::error::{AppError, AppResult};

/// Progress event payload — `page` is 1-based to match the UX copy
/// ("page 12 / 87").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub page: u32,
    pub total: u32,
}

/// Final result returned to the caller (and serialized to the webview).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JobResult {
    /// Actual on-disk output directory after `unique_path` resolution.
    pub output_dir: PathBuf,
    pub page_count: u32,
    pub duration_ms: u64,
}

/// Run a full PDF → Images job: read the file, render every page, write each
/// to `{stem}_pages/page_NNN.{ext}`, surfacing progress through `on_progress`.
///
/// `cancel` is checked by [`convert`] between pages — a cancel triggered
/// mid-job leaves already-written pages on disk.
///
/// Errors propagate without partial-state cleanup:
/// - input missing / unreadable → mapped via [`io_to_app_err`]
/// - convert errors ([`AppError::Encrypted`], `ProcessingFailed`) bubble unchanged
/// - `on_progress` returning `Err` halts the job and propagates
pub fn run_job<F>(
    input: &Path,
    opts: &Opts,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();
    let bytes = std::fs::read(input).map_err(|err| io_to_app_err(input, err))?;
    let target = derive_output_dir(input);
    let format = opts.format;

    let mut writer: Option<PageWriter> = None;
    let summary = convert(
        &bytes,
        opts,
        |page| {
            if writer.is_none() {
                writer = Some(PageWriter::create(&target, format, page.total)?);
            }
            let Some(w) = writer.as_ref() else {
                return Err(AppError::ProcessingFailed {
                    detail: "internal: writer init silently lost".into(),
                });
            };
            w.write_page(&page)?;
            on_progress(Progress {
                page: page.index.saturating_add(1),
                total: page.total,
            })?;
            Ok(())
        },
        cancel,
    )?;

    let output_dir = writer
        .as_ref()
        .map(|w| w.dir().to_path_buf())
        .unwrap_or(target);
    Ok(JobResult {
        output_dir,
        page_count: summary.page_count,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// `{parent}/{stem}_pages` — pure path math, no disk touch.
fn derive_output_dir(input: &Path) -> PathBuf {
    let mut name = input.file_stem().map(ToOwned::to_owned).unwrap_or_default();
    name.push("_pages");
    input
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
    use crate::tools::pdf_to_images::Format;
    use std::cell::RefCell;
    use std::fs;

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(format!("tests/fixtures/{name}")).unwrap_or_else(|_| panic!("read fixture {name}"))
    }

    fn opts(format: Format, dpi: u32) -> Opts {
        Opts { format, dpi }
    }

    #[test]
    fn happy_path_three_page_pdf_writes_files_and_streams_progress_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("doc.pdf");
        fs::write(&input, fixture("three-page.pdf")).unwrap();
        let cancel = CancellationToken::new();
        let progress = RefCell::new(Vec::new());

        let result = run_job(&input, &opts(Format::Png, 72), &cancel, |p| {
            progress.borrow_mut().push(p);
            Ok(())
        })
        .unwrap();

        assert_eq!(result.page_count, 3);
        assert_eq!(result.output_dir, tmp.path().join("doc_pages"));
        assert!(result.output_dir.join("page_001.png").is_file());
        assert!(result.output_dir.join("page_002.png").is_file());
        assert!(result.output_dir.join("page_003.png").is_file());
        assert_eq!(
            progress.into_inner(),
            vec![
                Progress { page: 1, total: 3 },
                Progress { page: 2, total: 3 },
                Progress { page: 3, total: 3 },
            ],
        );
    }

    #[test]
    fn cancellation_after_first_page_returns_cancelled_with_partial_output_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("doc.pdf");
        fs::write(&input, fixture("three-page.pdf")).unwrap();
        let cancel = CancellationToken::new();
        let cancel_signal = cancel.clone();
        let calls = RefCell::new(0u32);

        let result = run_job(&input, &opts(Format::Png, 72), &cancel, |_p| {
            *calls.borrow_mut() += 1;
            if *calls.borrow() == 1 {
                cancel_signal.cancel();
            }
            Ok(())
        });

        assert!(matches!(result, Err(AppError::Cancelled)), "got {result:?}");
        let out_dir = tmp.path().join("doc_pages");
        assert!(out_dir.join("page_001.png").is_file());
        assert!(!out_dir.join("page_002.png").exists());
        assert!(!out_dir.join("page_003.png").exists());
    }

    #[test]
    fn missing_input_returns_file_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("nonexistent.pdf");
        let cancel = CancellationToken::new();

        let result = run_job(&input, &opts(Format::Png, 72), &cancel, |_| Ok(()));

        match result {
            Err(AppError::FileNotFound { path }) => {
                assert!(path.contains("nonexistent.pdf"), "path was {path}");
            }
            other => panic!("expected FileNotFound, got {other:?}"),
        }
    }

    #[test]
    fn encrypted_pdf_returns_encrypted_and_leaves_no_output_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("locked.pdf");
        fs::write(&input, fixture("encrypted.pdf")).unwrap();
        let cancel = CancellationToken::new();

        let result = run_job(&input, &opts(Format::Png, 72), &cancel, |_| Ok(()));

        assert!(matches!(result, Err(AppError::Encrypted)));
        // Lazy-writer contract: no on_page fired → no folder created.
        assert!(!tmp.path().join("locked_pages").exists());
    }

    #[test]
    fn output_dir_collision_routes_through_unique_path_and_leaves_existing_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("doc.pdf");
        fs::write(&input, fixture("three-page.pdf")).unwrap();
        let existing = tmp.path().join("doc_pages");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("preexisting.txt"), b"keep me").unwrap();
        let cancel = CancellationToken::new();

        let result = run_job(&input, &opts(Format::Png, 72), &cancel, |_| Ok(())).unwrap();

        assert_eq!(result.output_dir, tmp.path().join("doc_pages (1)"));
        assert!(result.output_dir.join("page_001.png").is_file());
        assert_eq!(
            fs::read(existing.join("preexisting.txt")).unwrap(),
            b"keep me"
        );
    }

    #[test]
    fn on_progress_error_halts_and_propagates() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("doc.pdf");
        fs::write(&input, fixture("three-page.pdf")).unwrap();
        let cancel = CancellationToken::new();

        let result = run_job(&input, &opts(Format::Png, 72), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "test halt".into(),
            })
        });

        match result {
            Err(AppError::ProcessingFailed { detail }) => assert_eq!(detail, "test halt"),
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }

    #[test]
    fn derive_output_dir_appends_pages_suffix_to_stem_alongside_input() {
        assert_eq!(
            derive_output_dir(Path::new("/tmp/report.pdf")),
            PathBuf::from("/tmp/report_pages"),
        );
        assert_eq!(
            derive_output_dir(Path::new("local.pdf")),
            PathBuf::from("local_pages"),
        );
        assert_eq!(
            derive_output_dir(Path::new("./sub/doc.PDF")),
            PathBuf::from("./sub/doc_pages"),
        );
    }
}
