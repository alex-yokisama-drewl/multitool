//! Image Compress — batch orchestrator.
//!
//! Same shape as [`super::super::image_format_converter::job::run_job`]:
//! walks `inputs` in picked order, calls [`super::convert::compress_one`]
//! per file, writes successful outputs to disk via [`crate::fs::unique_path`],
//! and streams per-file events through `on_progress`. Per-file failures land
//! in the skipped list and **do not abort** the job (skip-and-continue).
//!
//! Cancellation is between files only — a JPEG/WebP encode for a single
//! image is fast enough that mid-file cancellation isn't useful and would
//! complicate cleanup. The top-of-loop checkpoint aborts before the next
//! file is read; the file in flight finishes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{compress_one, CompressedOutput, Opts};
use crate::error::{AppError, AppResult};
use crate::fs::unique_path;

/// Per-file event streamed to the UI as the job progresses.
///
/// `index` is 0-based in input-list order; `total` is the picked file count
/// and stays constant. Every file fires `Started` first, then either
/// `Succeeded` (with the before/after byte counts so the UI can render the
/// per-file size delta) or `Skipped`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    Started {
        index: u32,
        total: u32,
        source: PathBuf,
    },
    Succeeded {
        index: u32,
        total: u32,
        source: PathBuf,
        output: PathBuf,
        original_bytes: u64,
        compressed_bytes: u64,
    },
    Skipped {
        index: u32,
        total: u32,
        source: PathBuf,
        error: AppError,
    },
}

/// One entry per successfully-compressed file. Mirrors the fields the UI
/// shows in the done view (source → output: 2.4 MB → 1.1 MB).
#[derive(Clone, Debug, Serialize)]
pub struct CompressedFile {
    pub source: PathBuf,
    pub output: PathBuf,
    pub original_bytes: u64,
    pub compressed_bytes: u64,
}

/// One entry per file that failed and was skipped. The orchestrator
/// continues with the next file.
#[derive(Clone, Debug, Serialize)]
pub struct SkippedFile {
    pub source: PathBuf,
    pub error: AppError,
}

/// Returned from [`run_job`] on a completed (non-cancelled, non-orchestrator-
/// error) run. Even when every file was skipped, the result is `Ok(JobResult)`
/// with `success_count = 0`.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    pub success_count: u32,
    pub skip_count: u32,
    /// Per-file size deltas, in input-list order, for everything that
    /// succeeded. Drives the done view.
    pub outputs: Vec<CompressedFile>,
    pub skipped: Vec<SkippedFile>,
    /// Full path of the first successful output (handy for "reveal in
    /// folder"). `None` when no file succeeded.
    pub first_output_path: Option<PathBuf>,
    pub duration_ms: u64,
}

/// Run the full batch end-to-end. Returns once every file has been processed
/// or until cancellation triggers between files.
///
/// Per-file failures accumulate in [`JobResult::skipped`]; the job returns
/// `Ok(JobResult)` unless one of the orchestrator-level aborts fires:
/// - empty `inputs` slice → `AppError::ProcessingFailed`
/// - cancellation triggered before / between files → `AppError::Cancelled`
pub fn run_job<F>(
    inputs: &[PathBuf],
    opts: &Opts,
    cancel: &CancellationToken,
    mut on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();
    let total = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
    if total == 0 {
        return Err(AppError::ProcessingFailed {
            detail: "no images to compress".into(),
        });
    }

    let mut outputs: Vec<CompressedFile> = Vec::new();
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
            Ok(file) => {
                if first_output_path.is_none() {
                    first_output_path = Some(file.output.clone());
                }
                on_progress(Progress::Succeeded {
                    index,
                    total,
                    source: source.clone(),
                    output: file.output.clone(),
                    original_bytes: file.original_bytes,
                    compressed_bytes: file.compressed_bytes,
                })?;
                outputs.push(file);
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
        success_count: u32::try_from(outputs.len()).unwrap_or(u32::MAX),
        skip_count: u32::try_from(skipped.len()).unwrap_or(u32::MAX),
        outputs,
        skipped,
        first_output_path,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Single-file pipeline: stat (for `original_bytes`) → read → compress →
/// derive output path with the strategy's target extension → `unique_path`
/// → write → return the size delta.
fn process_one(source: &Path, opts: &Opts) -> AppResult<CompressedFile> {
    let metadata = fs::metadata(source).map_err(|err| io_to_app_err(source, &err))?;
    let original_bytes = metadata.len();
    let bytes = fs::read(source).map_err(|err| io_to_app_err(source, &err))?;
    let source_ext = source
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let CompressedOutput {
        bytes: encoded,
        target_extension,
    } = compress_one(&source_ext, &bytes, opts)?;

    let target_path = derive_output_path(source, &target_extension);
    let final_path = unique_path(&target_path).map_err(|err| io_to_app_err(&target_path, &err))?;
    fs::write(&final_path, &encoded).map_err(|err| io_to_app_err(&final_path, &err))?;
    let compressed_bytes = encoded.len() as u64;
    Ok(CompressedFile {
        source: source.to_path_buf(),
        output: final_path,
        original_bytes,
        compressed_bytes,
    })
}

/// `{stem}_compressed.{target_extension}` next to the source, **before**
/// `unique_path` resolution. The target extension comes from the
/// per-source-format strategy — `.jpg` for a JPEG source, `.webp` for a
/// PNG/BMP/TIFF source, etc.
fn derive_output_path(source: &Path, target_extension: &str) -> PathBuf {
    let parent = source.parent();
    let mut name = source.file_stem().map(|s| s.to_owned()).unwrap_or_default();
    name.push("_compressed.");
    name.push(target_extension);
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

    fn opts(quality: f64) -> Opts {
        Opts { quality }
    }

    // --- Happy paths ---

    #[test]
    fn happy_mixed_batch_writes_outputs_with_per_strategy_extensions() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![
            copy_fixture_to(dir.path(), "blue.jpg"),
            copy_fixture_to(dir.path(), "red.png"),
            copy_fixture_to(dir.path(), "green.webp"),
        ];

        let events = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &opts(0.8), &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("run_job ok");

        assert_eq!(result.success_count, 3);
        assert_eq!(result.skip_count, 0);
        assert_eq!(result.outputs.len(), 3);

        // JPEG → .jpg (preserves source spelling)
        assert!(dir.path().join("blue_compressed.jpg").exists());
        // PNG → .webp
        assert!(dir.path().join("red_compressed.webp").exists());
        // WebP → .webp
        assert!(dir.path().join("green_compressed.webp").exists());

        // Per-file byte counts are populated.
        for f in &result.outputs {
            assert!(f.original_bytes > 0);
            assert!(f.compressed_bytes > 0);
        }

        // first_output_path tracks the first SUCCESSFUL output (= first input
        // here, since none were skipped).
        assert_eq!(
            result.first_output_path.as_deref(),
            Some(dir.path().join("blue_compressed.jpg").as_path())
        );

        // 3 inputs × 2 events (Started + Succeeded) = 6 events.
        assert_eq!(events.borrow().len(), 6);
    }

    #[test]
    fn outputs_carry_original_and_compressed_byte_counts() {
        // Verifies the done-view contract: original_bytes mirrors the file
        // metadata, compressed_bytes mirrors the written file size, and both
        // are non-zero. Doesn't assert compressed < original — that depends
        // on the source (a tiny already-encoded JPEG can bloat at any
        // quality). The lo-vs-hi quality comparison lives in convert.rs.
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "blue.jpg");
        let original_size = fs::metadata(&input).unwrap().len();
        let cancel = CancellationToken::new();
        let result = run_job(
            std::slice::from_ref(&input),
            &opts(0.8),
            &cancel,
            |_| Ok(()),
        )
        .expect("compress ok");
        let f = &result.outputs[0];
        assert_eq!(f.original_bytes, original_size);
        assert!(f.compressed_bytes > 0);
        assert_eq!(
            f.compressed_bytes,
            fs::metadata(&f.output).unwrap().len(),
            "compressed_bytes should match the written file"
        );
    }

    // --- Skip-and-continue ---

    #[test]
    fn mid_batch_garbage_file_is_skipped_and_others_succeed() {
        let dir = TempDir::new().expect("tempdir");
        let bad = dir.path().join("corrupt.jpg");
        fs::write(&bad, b"this is not a JPEG").expect("write garbage");
        let inputs = vec![
            copy_fixture_to(dir.path(), "blue.jpg"),
            bad.clone(),
            copy_fixture_to(dir.path(), "red.png"),
        ];

        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &opts(0.8), &cancel, |_| Ok(()))
            .expect("job continues past garbage file");
        assert_eq!(result.success_count, 2);
        assert_eq!(result.skip_count, 1);
        assert_eq!(result.skipped[0].source, bad);
        assert!(matches!(
            result.skipped[0].error,
            AppError::UnsupportedFormat { .. } | AppError::ProcessingFailed { .. }
        ));
        // Outputs from the good files were still written.
        assert!(dir.path().join("blue_compressed.jpg").exists());
        assert!(dir.path().join("red_compressed.webp").exists());
    }

    #[test]
    fn missing_input_file_is_skipped_as_file_not_found() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("does_not_exist.png");
        let inputs = vec![missing.clone()];
        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &opts(0.8), &cancel, |_| Ok(()))
            .expect("job ok with single missing input");
        assert_eq!(result.success_count, 0);
        assert_eq!(result.skip_count, 1);
        assert!(matches!(
            result.skipped[0].error,
            AppError::FileNotFound { .. }
        ));
    }

    // --- Cancellation ---

    #[test]
    fn cancel_before_any_file_returns_cancelled_with_no_writes() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![copy_fixture_to(dir.path(), "blue.jpg")];
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&inputs, &opts(0.8), &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0, "no progress events fire pre-cancel");
        assert!(!dir.path().join("blue_compressed.jpg").exists());
    }

    #[test]
    fn cancel_between_files_preserves_already_written_outputs() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![
            copy_fixture_to(dir.path(), "blue.jpg"),
            copy_fixture_to(dir.path(), "red.png"),
        ];
        let cancel = CancellationToken::new();

        let result = run_job(&inputs, &opts(0.8), &cancel, |progress| {
            // Cancel right after the first file's success event so the loop's
            // top-of-iteration checkpoint trips on the next iteration.
            if let Progress::Succeeded { index: 0, .. } = progress {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        // First file written; second one not started.
        assert!(dir.path().join("blue_compressed.jpg").exists());
        assert!(!dir.path().join("red_compressed.webp").exists());
    }

    // --- Collision via unique_path ---

    #[test]
    fn output_name_collision_routes_through_unique_path() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "blue.jpg");
        // Pre-create a file at the would-be output path.
        let placeholder = dir.path().join("blue_compressed.jpg");
        fs::write(&placeholder, b"placeholder").expect("write placeholder");
        let placeholder_bytes = fs::read(&placeholder).unwrap();

        let cancel = CancellationToken::new();
        let result = run_job(&[input], &opts(0.8), &cancel, |_| Ok(()))
            .expect("compress ok despite collision");
        let resolved = dir.path().join("blue_compressed (1).jpg");
        assert_eq!(result.outputs[0].output, resolved);
        assert!(resolved.exists());
        // Placeholder untouched.
        assert_eq!(fs::read(&placeholder).unwrap(), placeholder_bytes);
    }

    // --- Per-source-format dispatch through the orchestrator ---

    #[test]
    fn png_input_produces_webp_output_path() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "red.png");
        let cancel = CancellationToken::new();
        let result = run_job(&[input], &opts(0.8), &cancel, |_| Ok(())).expect("compress ok");
        assert!(result.outputs[0]
            .output
            .to_string_lossy()
            .ends_with("red_compressed.webp"));
    }

    // --- on_progress error aborts ---

    #[test]
    fn on_progress_error_aborts_the_job() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![copy_fixture_to(dir.path(), "blue.jpg")];
        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &opts(0.8), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }

    // --- Empty inputs ---

    #[test]
    fn empty_inputs_yield_processing_failed() {
        let cancel = CancellationToken::new();
        let result = run_job(&[], &opts(0.8), &cancel, |_| Ok(()));
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert_eq!(detail, "no images to compress");
            }
            other => panic!("expected ProcessingFailed for empty inputs, got {other:?}"),
        }
    }
}
