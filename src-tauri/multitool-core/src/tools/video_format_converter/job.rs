//! Batch orchestrator for the Video Format Converter.
//!
//! Mirrors [`super::super::audio_format_converter::job`] — same `Started /
//! Succeeded / Skipped` event shape, same per-file skip-and-continue rule.
//! New for video: a [`Progress::FileProgress`] variant that streams
//! mid-encode 0..=1 progress for the file currently in-flight, sourced
//! from the ffmpeg shim's `out_time_us / probed_duration` math.
//!
//! Cancellation semantics:
//! - Between files: checked at the top of each iteration → returns
//!   `AppError::Cancelled`.
//! - Mid-encode: [`crate::ffmpeg::run`] reaps the child and returns
//!   `AppError::Cancelled`, which propagates from [`super::convert`].
//!   The job returns `Cancelled` (does **not** record the in-flight
//!   source as Skipped — it was user-cancelled, not a per-file failure).
//!   Already-written outputs from prior files stay on disk.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{convert, Opts};
use crate::error::{AppError, AppResult};

/// Per-file event streamed to the UI as the job progresses.
///
/// `index` is 0-based in input-list order; `total` is the picked file
/// count and is constant across the job. Each file fires `Started`
/// first, then zero or more `FileProgress`, then either `Succeeded`
/// or `Skipped`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to convert this file. Emitted once per file.
    Started {
        index: u32,
        total: u32,
        source: PathBuf,
    },
    /// Mid-encode progress for the file currently in-flight. `fraction`
    /// is in `[0.0, 1.0]`. Emitted at most ~4× / sec (the ffmpeg shim
    /// throttles to 250ms).
    FileProgress {
        index: u32,
        total: u32,
        source: PathBuf,
        fraction: f64,
    },
    /// File converted; output written at `output`.
    Succeeded {
        index: u32,
        total: u32,
        source: PathBuf,
        output: PathBuf,
    },
    /// Per-file failure; the job continues with the next file.
    Skipped {
        index: u32,
        total: u32,
        source: PathBuf,
        error: AppError,
    },
}

/// One entry in the final summary's `skipped` list.
#[derive(Clone, Debug, Serialize)]
pub struct SkippedFile {
    pub source: PathBuf,
    pub error: AppError,
}

/// Result of a completed batch run.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    pub success_count: u32,
    pub skip_count: u32,
    pub skipped: Vec<SkippedFile>,
    /// Path of the first successful output file (handy for "reveal in
    /// folder"). `None` when no file succeeded.
    pub first_output_path: Option<PathBuf>,
    pub duration_ms: u64,
}

/// Drive the batch end-to-end. Returns once every file has been processed
/// or until cancellation triggers (between files or mid-encode).
///
/// Per-file failures land in [`JobResult::skipped`]; the job itself
/// returns `Ok(JobResult)` unless one of the orchestrator-level aborts
/// fires:
/// - empty `inputs` slice → `AppError::ProcessingFailed`
/// - cancellation (between or mid-file) → `AppError::Cancelled`
/// - the caller's `on_progress` returns `Err(...)` → propagated unchanged
pub fn run_job<F>(
    inputs: &[PathBuf],
    opts: &Opts,
    cancel: &CancellationToken,
    on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();
    let total = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
    if total == 0 {
        return Err(AppError::ProcessingFailed {
            detail: "no video files to convert".into(),
        });
    }

    // RefCell so the inner per-file progress callback (a closure passed
    // *into* `convert`) and the outer orchestrator both call the caller's
    // `on_progress` emitter. The borrow is short-lived and non-reentrant
    // in either direction.
    let on_progress = RefCell::new(on_progress);

    let mut success_count: u32 = 0;
    let mut skipped: Vec<SkippedFile> = Vec::new();
    let mut first_output_path: Option<PathBuf> = None;

    for (idx, source) in inputs.iter().enumerate() {
        let index = u32::try_from(idx).unwrap_or(u32::MAX);
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        (on_progress.borrow_mut())(Progress::Started {
            index,
            total,
            source: source.clone(),
        })?;

        // If a FileProgress emit fails (caller returned Err), record the
        // error here and stop calling the emitter for this file. We
        // propagate the captured error after `convert` returns. We can't
        // bubble it directly because the inner closure can't return a
        // Result — ffmpeg progress callbacks are infallible by design,
        // matching `crate::ffmpeg::run`'s `FnMut(FfmpegProgress)` shape.
        let progress_err: RefCell<Option<AppError>> = RefCell::new(None);
        let convert_result = convert(
            source,
            opts,
            |fraction| {
                if progress_err.borrow().is_some() {
                    return;
                }
                let res = (on_progress.borrow_mut())(Progress::FileProgress {
                    index,
                    total,
                    source: source.clone(),
                    fraction,
                });
                if let Err(err) = res {
                    *progress_err.borrow_mut() = Some(err);
                }
            },
            cancel,
        );

        if let Some(err) = progress_err.into_inner() {
            return Err(err);
        }

        match convert_result {
            Ok(output) => {
                success_count = success_count.saturating_add(1);
                if first_output_path.is_none() {
                    first_output_path = Some(output.clone());
                }
                (on_progress.borrow_mut())(Progress::Succeeded {
                    index,
                    total,
                    source: source.clone(),
                    output,
                })?;
            }
            // User-triggered mid-encode cancel ends the whole batch. The
            // in-flight file's partial output was already cleaned up by
            // `convert`; previously-written outputs stay on disk.
            Err(AppError::Cancelled) => return Err(AppError::Cancelled),
            Err(error) => {
                skipped.push(SkippedFile {
                    source: source.clone(),
                    error: error.clone(),
                });
                (on_progress.borrow_mut())(Progress::Skipped {
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

#[cfg(test)]
mod tests {
    use super::super::convert::TargetFormat;
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use tempfile::TempDir;

    fn opts(target_format: TargetFormat) -> Opts {
        Opts { target_format }
    }

    /// Synthesize a tiny mp4 clip via the bundled ffmpeg so we have a
    /// real video on disk to feed the converter. Avoids committing
    /// binary fixtures — the bundled ffmpeg is always available in any
    /// environment where these tests can run (built or not, the build
    /// script puts it in `OUT_DIR`).
    fn synth_clip(dir: &Path, name: &str, duration: u32) -> PathBuf {
        let out = dir.join(name);
        let out_str = out.to_str().expect("utf-8 tempdir");
        let args = [
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={duration}:size=64x64:rate=10"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={duration}"),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-shortest",
            out_str,
        ];
        crate::ffmpeg::run(args, |_| {}, &CancellationToken::new())
            .expect("synthesize test clip via bundled ffmpeg");
        out
    }

    #[test]
    fn empty_inputs_yield_processing_failed() {
        let cancel = CancellationToken::new();
        let result = run_job(&[], &opts(TargetFormat::Mp4), &cancel, |_| Ok(()));
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert_eq!(detail, "no video files to convert");
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }

    #[test]
    fn happy_path_single_mp4_to_mp4_writes_output_with_converted_suffix() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 1);

        let events = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let result = run_job(
            std::slice::from_ref(&input),
            &opts(TargetFormat::Mp4),
            &cancel,
            |p| {
                events.borrow_mut().push(p);
                Ok(())
            },
        )
        .expect("run_job ok");

        assert_eq!(result.success_count, 1);
        assert_eq!(result.skip_count, 0);
        let expected = dir.path().join("input_converted.mp4");
        assert_eq!(result.first_output_path.as_ref(), Some(&expected));
        assert!(expected.exists());

        // Should have at least Started + Succeeded; usually a few
        // FileProgress in between.
        let kinds: Vec<&'static str> = events
            .borrow()
            .iter()
            .map(|p| match p {
                Progress::Started { .. } => "Started",
                Progress::FileProgress { .. } => "FileProgress",
                Progress::Succeeded { .. } => "Succeeded",
                Progress::Skipped { .. } => "Skipped",
            })
            .collect();
        assert_eq!(kinds.first().copied(), Some("Started"));
        assert_eq!(kinds.last().copied(), Some("Succeeded"));
    }

    #[test]
    fn batch_writes_all_three_target_formats() {
        // One input, three runs into three different formats. Cheaper
        // than three different sources and covers the full codec set.
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "src.mp4", 1);
        let cancel = CancellationToken::new();

        for target in [TargetFormat::Mp4, TargetFormat::Webm, TargetFormat::Mkv] {
            let result = run_job(std::slice::from_ref(&input), &opts(target), &cancel, |_| {
                Ok(())
            })
            .unwrap_or_else(|err| panic!("run_job for {target:?} failed: {err:?}"));
            assert_eq!(result.success_count, 1, "{target:?} should succeed");
            let out = result.first_output_path.expect("first output");
            assert_eq!(
                out.extension().and_then(|s| s.to_str()),
                Some(target.extension())
            );
            assert!(out.exists());
            assert!(std::fs::metadata(&out).unwrap().len() > 0);
        }
    }

    #[test]
    fn mid_batch_garbage_file_is_skipped_and_others_succeed() {
        let dir = TempDir::new().unwrap();
        let good_1 = synth_clip(dir.path(), "good_1.mp4", 1);
        let bad = dir.path().join("bad.mp4");
        std::fs::write(&bad, b"this is not an mp4 at all").unwrap();
        let good_2 = synth_clip(dir.path(), "good_2.mp4", 1);

        let cancel = CancellationToken::new();
        let result = run_job(
            &[good_1, bad.clone(), good_2],
            &opts(TargetFormat::Webm),
            &cancel,
            |_| Ok(()),
        )
        .expect("job continues past per-file failure");

        assert_eq!(result.success_count, 2);
        assert_eq!(result.skip_count, 1);
        assert_eq!(result.skipped[0].source, bad);
        // ffmpeg's "Invalid data found when processing input" maps to
        // ProcessingFailed via the shim's stderr-tail mechanism.
        assert!(matches!(
            result.skipped[0].error,
            AppError::ProcessingFailed { .. }
        ));
    }

    #[test]
    fn missing_input_is_skipped_as_file_not_found() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope.mp4");
        let cancel = CancellationToken::new();

        let result = run_job(
            std::slice::from_ref(&missing),
            &opts(TargetFormat::Mp4),
            &cancel,
            |_| Ok(()),
        )
        .expect("job ok with single missing input");

        assert_eq!(result.skip_count, 1);
        assert!(matches!(
            result.skipped[0].error,
            AppError::FileNotFound { .. }
        ));
    }

    #[test]
    fn cancel_before_any_file_returns_cancelled_with_no_writes() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 1);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&[input], &opts(TargetFormat::Mp4), &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0);
        assert!(!dir.path().join("input_converted.mp4").exists());
    }

    #[test]
    fn cancel_mid_encode_returns_cancelled_and_deletes_partial_output() {
        let dir = TempDir::new().unwrap();
        // 30-second source so the encode runs long enough to cancel
        // mid-stream. ultrafast encode of 64×64 still takes appreciable
        // time at this length.
        let input = synth_clip(dir.path(), "long.mp4", 30);
        let cancel = CancellationToken::new();

        let result = run_job(&[input], &opts(TargetFormat::Webm), &cancel, |p| {
            // Cancel once we see any mid-file progress — proves we're
            // actually mid-encode, not still spawning ffmpeg.
            if matches!(p, Progress::FileProgress { .. }) {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        // Partial output should have been cleaned up by `convert`.
        assert!(!dir.path().join("long_converted.webm").exists());
    }

    #[test]
    fn cancel_between_files_preserves_already_written_outputs() {
        let dir = TempDir::new().unwrap();
        let input_1 = synth_clip(dir.path(), "a.mp4", 1);
        let input_2 = synth_clip(dir.path(), "b.mp4", 1);
        let cancel = CancellationToken::new();

        let result = run_job(
            &[input_1, input_2],
            &opts(TargetFormat::Mkv),
            &cancel,
            |progress| {
                if let Progress::Succeeded { index: 0, .. } = progress {
                    cancel.cancel();
                }
                Ok(())
            },
        );
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert!(dir.path().join("a_converted.mkv").exists());
        assert!(!dir.path().join("b_converted.mkv").exists());
    }

    #[test]
    fn output_collision_routes_through_unique_path() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mp4", 1);
        // Pre-create the would-be output name so unique_path has to
        // append " (1)".
        std::fs::write(dir.path().join("clip_converted.mp4"), b"placeholder").unwrap();
        let placeholder_bytes = std::fs::read(dir.path().join("clip_converted.mp4")).unwrap();

        let cancel = CancellationToken::new();
        let result = run_job(
            std::slice::from_ref(&input),
            &opts(TargetFormat::Mp4),
            &cancel,
            |_| Ok(()),
        )
        .expect("collision-resolved write ok");

        assert_eq!(result.success_count, 1);
        let resolved = dir.path().join("clip_converted (1).mp4");
        assert_eq!(result.first_output_path.as_ref(), Some(&resolved));
        assert!(resolved.exists());
        // Pre-existing file untouched.
        assert_eq!(
            std::fs::read(dir.path().join("clip_converted.mp4")).unwrap(),
            placeholder_bytes
        );
    }

    #[test]
    fn on_progress_error_aborts_the_job() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mp4", 1);
        let cancel = CancellationToken::new();

        let result = run_job(&[input], &opts(TargetFormat::Mp4), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }
}
