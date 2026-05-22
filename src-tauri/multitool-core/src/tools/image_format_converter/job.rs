//! Batch orchestrator for the Image Format Converter.
//!
//! [`run_job`] walks the picked input files, calls [`super::convert::convert_one`]
//! per file, writes successful outputs to disk via [`crate::fs::unique_path`],
//! and streams progress through the `on_progress` callback. Per-file failures
//! land in the skipped list and **do not abort** the job — that's the
//! "skip + continue" rule from the brief.
//!
//! Cancellation does abort the job (with `AppError::Cancelled`); so does an
//! empty input slice (`AppError::ProcessingFailed`). Per-file `FileNotFound`,
//! decode failures, and alpha-handling refusals are all skips.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{convert_one, Opts, TargetFormat};
use crate::error::{AppError, AppResult};
use crate::fs::unique_path;

/// Per-file event streamed to the UI as the job progresses.
///
/// `index` is 0-based in input-list order; `total` is the picked file count
/// and is constant across the job. Each file fires `Started` first, then
/// either `Succeeded` or `Skipped`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to read + convert this file.
    Started {
        index: u32,
        total: u32,
        source: PathBuf,
    },
    /// File converted and written to disk at `output`. `warnings` collects
    /// non-fatal per-file notes (animated GIF first-frame, SVG text, …).
    Succeeded {
        index: u32,
        total: u32,
        source: PathBuf,
        output: PathBuf,
        warnings: Vec<String>,
    },
    /// File skipped. The job continues with the next file.
    Skipped {
        index: u32,
        total: u32,
        source: PathBuf,
        error: AppError,
    },
}

/// One entry in the final summary's `skipped` list. Mirrors the relevant
/// fields of [`Progress::Skipped`] so the UI can render either source.
#[derive(Clone, Debug, Serialize)]
pub struct SkippedFile {
    pub source: PathBuf,
    pub error: AppError,
}

/// Returned from [`run_job`] on a (non-cancelled, non-orchestrator-error) run.
/// Always represents a completed batch — even when every file was skipped,
/// the job result is `Ok(JobResult)` with `success_count = 0`.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    pub success_count: u32,
    pub skip_count: u32,
    pub skipped: Vec<SkippedFile>,
    /// Full path of the first successful output file (handy for "reveal in
    /// folder" UX — pass it straight to `revealItemInDir`, which opens the
    /// parent directory and highlights the file). `None` when no file
    /// succeeded.
    pub first_output_path: Option<PathBuf>,
    pub duration_ms: u64,
}

/// Run the full batch end-to-end. Returns once every file has been processed
/// or until cancellation triggers between files.
///
/// Per-file failures are accumulated into [`JobResult::skipped`]; the job
/// itself returns `Ok(JobResult)` unless one of the orchestrator-level
/// aborts fires:
/// - empty `inputs` slice → `AppError::ProcessingFailed`
/// - cancellation triggered before / between files → `AppError::Cancelled`
pub fn run_job<F>(
    inputs: &[PathBuf],
    opts: &Opts,
    mut on_progress: F,
    cancel: &CancellationToken,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();
    let total = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
    if total == 0 {
        return Err(AppError::ProcessingFailed {
            detail: "no images to convert".into(),
        });
    }

    let mut success_count: u32 = 0;
    let mut skipped: Vec<SkippedFile> = Vec::new();
    let mut first_output_path: Option<PathBuf> = None;

    for (idx, source) in inputs.iter().enumerate() {
        let index = u32::try_from(idx).unwrap_or(u32::MAX);
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        on_progress(Progress::Started {
            index,
            total,
            source: source.clone(),
        })?;

        match process_one(source, opts) {
            Ok((output, warnings)) => {
                success_count = success_count.saturating_add(1);
                if first_output_path.is_none() {
                    first_output_path = Some(output.clone());
                }
                on_progress(Progress::Succeeded {
                    index,
                    total,
                    source: source.clone(),
                    output,
                    warnings,
                })?;
            }
            Err(error) => {
                skipped.push(SkippedFile {
                    source: source.clone(),
                    error: error.clone(),
                });
                on_progress(Progress::Skipped {
                    index,
                    total,
                    source: source.clone(),
                    error,
                })?;
            }
        }
    }

    Ok(JobResult {
        success_count,
        skip_count: u32::try_from(skipped.len()).unwrap_or(u32::MAX),
        skipped,
        first_output_path,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Single-file pipeline: read bytes, derive output path, convert, write.
///
/// All per-file failure paths funnel through here as `Err(AppError::*)`. The
/// orchestrator catches and turns them into [`Progress::Skipped`] entries.
fn process_one(source: &Path, opts: &Opts) -> AppResult<(PathBuf, Vec<String>)> {
    let bytes = fs::read(source).map_err(|err| io_to_app_err(source, &err))?;
    let source_ext = source
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let encoded = convert_one(&source_ext, &bytes, opts)?;

    let target_path = derive_output_path(source, opts.target_format);
    let final_path = unique_path(&target_path).map_err(|err| io_to_app_err(&target_path, &err))?;
    fs::write(&final_path, &encoded.bytes).map_err(|err| io_to_app_err(&final_path, &err))?;
    Ok((final_path, encoded.warnings))
}

/// Compute the desired output path for `source` under `target_format`,
/// **before** `unique_path` resolution. Strips the source extension and
/// appends the target format's canonical extension; collisions become the
/// caller's problem via `unique_path`.
fn derive_output_path(source: &Path, target_format: TargetFormat) -> PathBuf {
    let mut out = source.to_path_buf();
    out.set_extension(target_format.extension());
    out
}

/// Map `std::io::Error` for a specific path into the corresponding `AppError`
/// variant. `NotFound` and `PermissionDenied` get typed variants; anything
/// else lands in `ProcessingFailed` with the path embedded so the user can
/// identify which file failed in the skipped list.
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
    use super::super::convert::{AlphaHandling, SvgRasterSize};
    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn images_fixture_path(name: &str) -> PathBuf {
        PathBuf::from(format!("tests/fixtures/images/{name}"))
    }

    fn default_opts(target_format: TargetFormat) -> Opts {
        Opts {
            target_format,
            jpeg_quality: 85,
            alpha_handling: AlphaHandling::FlattenWhite,
            svg_raster_size: SvgRasterSize::LongestEdgePx(1024),
        }
    }

    /// Copy a fixture into `dir` so the orchestrator can write its output
    /// next to it (mirrors the real "output in the input's directory" flow).
    fn copy_fixture_to(dir: &Path, name: &str) -> PathBuf {
        let dst = dir.join(name);
        fs::copy(images_fixture_path(name), &dst).expect("copy fixture");
        dst
    }

    #[test]
    fn happy_path_three_file_batch_writes_outputs_and_reports_counts() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![
            copy_fixture_to(dir.path(), "red.png"),
            copy_fixture_to(dir.path(), "blue.jpg"),
            copy_fixture_to(dir.path(), "green.webp"),
        ];

        let events = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let result = run_job(
            &inputs,
            &default_opts(TargetFormat::Png),
            |p| {
                events.borrow_mut().push(p);
                Ok(())
            },
            &cancel,
        )
        .expect("run_job ok");

        assert_eq!(result.success_count, 3);
        assert_eq!(result.skip_count, 0);
        assert!(result.skipped.is_empty());
        // first_output_path is the FILE, not the directory. Asserting it
        // lives under the input dir is enough; the orchestrator picks the
        // first successful Succeeded.output.
        let first = result.first_output_path.as_deref().expect("first output");
        assert_eq!(first.parent(), Some(dir.path()));
        assert!(first.exists(), "{first:?} should exist");

        // 3 inputs × 2 events (Started + Succeeded) = 6 events.
        assert_eq!(events.borrow().len(), 6);

        // Every Succeeded.output should be on disk.
        for evt in events.borrow().iter() {
            if let Progress::Succeeded { output, .. } = evt {
                assert!(output.exists(), "{output:?} should exist");
            }
        }
    }

    #[test]
    fn mid_batch_unsupported_file_is_skipped_and_others_succeed() {
        let dir = TempDir::new().expect("tempdir");
        let bad_path = dir.path().join("garbage.png");
        fs::write(&bad_path, b"this is not a PNG").expect("write garbage");
        let inputs = vec![
            copy_fixture_to(dir.path(), "red.png"),
            bad_path.clone(),
            copy_fixture_to(dir.path(), "blue.jpg"),
        ];

        let cancel = CancellationToken::new();
        let result = run_job(
            &inputs,
            &default_opts(TargetFormat::Jpeg),
            |_| Ok(()),
            &cancel,
        )
        .expect("job should succeed despite mid-batch skip");

        assert_eq!(result.success_count, 2);
        assert_eq!(result.skip_count, 1);
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].source, bad_path);
        assert!(matches!(
            result.skipped[0].error,
            AppError::UnsupportedFormat { .. }
        ));
    }

    #[test]
    fn missing_input_file_is_skipped_as_file_not_found() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("does_not_exist.png");
        let inputs = vec![missing.clone()];

        let cancel = CancellationToken::new();
        let result = run_job(
            &inputs,
            &default_opts(TargetFormat::Png),
            |_| Ok(()),
            &cancel,
        )
        .expect("job ok with single missing input");
        assert_eq!(result.success_count, 0);
        assert_eq!(result.skip_count, 1);
        assert!(matches!(
            result.skipped[0].error,
            AppError::FileNotFound { .. }
        ));
    }

    #[test]
    fn cancel_before_any_file_returns_cancelled_with_no_writes() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![copy_fixture_to(dir.path(), "red.png")];
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(
            &inputs,
            &default_opts(TargetFormat::Png),
            |_| {
                *calls.borrow_mut() += 1;
                Ok(())
            },
            &cancel,
        );
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0, "no progress events fire pre-cancel");
        // Output file wasn't written.
        let expected = dir.path().join("red.png"); // same-format collision
                                                   // Only the copied source exists; no overwriting happened.
        assert!(expected.exists());
        // No `(1)` file from a successful run.
        assert!(!dir.path().join("red (1).png").exists());
    }

    #[test]
    fn cancel_between_files_preserves_already_written_outputs() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![
            copy_fixture_to(dir.path(), "red.png"),
            copy_fixture_to(dir.path(), "blue.jpg"),
        ];
        let cancel = CancellationToken::new();

        let result = run_job(
            &inputs,
            &default_opts(TargetFormat::Jpeg),
            |progress| {
                // Cancel right after the first file's success event so the
                // loop's next iteration trips the cancel checkpoint.
                if let Progress::Succeeded { index: 0, .. } = progress {
                    cancel.cancel();
                }
                Ok(())
            },
            &cancel,
        );
        assert!(matches!(result, Err(AppError::Cancelled)));
        // First file should have been written; second should not exist.
        assert!(dir.path().join("red.jpg").exists());
        assert!(
            !dir.path().join("blue.jpg").exists() || {
                // blue.jpg was the SOURCE, so it exists from the copy. Verify
                // it wasn't overwritten by checking the file is still the
                // original JPEG bytes (size differs from a fresh JPEG encode).
                let copied = fs::metadata(images_fixture_path("blue.jpg")).unwrap().len();
                let on_disk = fs::metadata(dir.path().join("blue.jpg")).unwrap().len();
                copied == on_disk
            }
        );
    }

    #[test]
    fn output_name_collision_routes_through_unique_path() {
        let dir = TempDir::new().expect("tempdir");
        // Same-format request: red.png input + PNG output collides with
        // itself (input and output share a path). unique_path should pick
        // "red (1).png" so the original red.png is preserved.
        let input = copy_fixture_to(dir.path(), "red.png");
        let original_bytes = fs::read(&input).unwrap();

        let cancel = CancellationToken::new();
        let result = run_job(
            std::slice::from_ref(&input),
            &default_opts(TargetFormat::Png),
            |_| Ok(()),
            &cancel,
        )
        .expect("same-format conversion ok");

        assert_eq!(result.success_count, 1);
        let new_path = dir.path().join("red (1).png");
        assert!(new_path.exists(), "expected red (1).png at {new_path:?}");
        // Original still untouched.
        assert_eq!(fs::read(&input).unwrap(), original_bytes);
    }

    #[test]
    fn on_progress_error_aborts_the_job() {
        // If the UI callback returns Err (e.g. event emission failed in the
        // shell), the orchestrator propagates it immediately.
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![copy_fixture_to(dir.path(), "red.png")];
        let cancel = CancellationToken::new();
        let result = run_job(
            &inputs,
            &default_opts(TargetFormat::Png),
            |_| {
                Err(AppError::ProcessingFailed {
                    detail: "emit failed".into(),
                })
            },
            &cancel,
        );
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }

    #[test]
    fn empty_inputs_yield_processing_failed() {
        let cancel = CancellationToken::new();
        let result = run_job(&[], &default_opts(TargetFormat::Png), |_| Ok(()), &cancel);
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert_eq!(detail, "no images to convert");
            }
            other => panic!("expected ProcessingFailed for empty inputs, got {other:?}"),
        }
    }
}
