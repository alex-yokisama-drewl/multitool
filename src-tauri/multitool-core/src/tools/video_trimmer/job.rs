//! Video Trimmer — single-file orchestrator.
//!
//! Mirrors [`super::super::audio_trimmer::job`] (one file in, one file
//! out, no skip-and-continue lane) but drives the ffmpeg-based
//! [`super::convert`] instead of a PCM round-trip. The single mid-run
//! `FileProgress` variant streams the re-encode's 0..=1 fraction,
//! sourced from the ffmpeg shim's `out_time_us / trim_duration` math —
//! the same shape the Video Format Converter uses.
//!
//! Cancellation:
//! - Before any work and again right after the `Started` emit (before the
//!   ffmpeg spawn) → returns `AppError::Cancelled`, nothing written.
//! - Mid-encode → [`crate::ffmpeg::run`] kills the child and returns
//!   `AppError::Cancelled`, which propagates from [`super::convert`],
//!   which has already unlinked the partial output.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{convert, Opts};
use crate::error::{AppError, AppResult};

/// Per-file event streamed to the UI as the trim progresses.
///
/// `Started` fires once when the re-encode begins; zero or more
/// `FileProgress` follow with the mid-encode 0..=1 fraction (throttled
/// to ~4/sec by the ffmpeg shim). On success the job resolves with
/// [`JobResult`]; on failure it rejects with an `AppError`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to trim the picked file. Emitted once.
    Started { source: PathBuf },
    /// Mid-encode progress. `fraction` is in `[0.0, 1.0]`.
    FileProgress { source: PathBuf, fraction: f64 },
}

/// Result of a completed trim.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    /// The path the trimmed clip was written to. Routes through
    /// `multitool_core::fs::unique_path`, so a same-name collision lands
    /// at `{stem}_trimmed (1).{ext}`.
    pub output: PathBuf,
    pub duration_ms: u64,
}

/// Run the trim end-to-end. Returns once the trimmed clip is written, or
/// earlier with an `AppError` if anything fails.
///
/// Aborts:
/// - cancellation (before/after `Started`, or mid-copy) → `Cancelled`
/// - the caller's `on_progress` returning `Err(...)` → propagated unchanged
pub fn run_job<F>(
    source: &Path,
    opts: &Opts,
    cancel: &CancellationToken,
    on_progress: F,
) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();

    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    // RefCell so the inner per-file progress closure (passed *into*
    // `convert`, which takes an infallible `FnMut(f64)`) and this
    // orchestrator both reach the caller's fallible emitter. The borrow
    // is short-lived and non-reentrant.
    let on_progress = RefCell::new(on_progress);

    (on_progress.borrow_mut())(Progress::Started {
        source: source.to_path_buf(),
    })?;

    // Cancel between Started and the ffmpeg spawn keeps the cancel path
    // deterministic — a re-encode of a sub-second window on a tiny test
    // clip can otherwise finish before the shim's read loop notices the
    // token.
    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    // If a FileProgress emit fails, stash the error and stop emitting for
    // the rest of the run; propagate it after `convert` returns (the inner
    // closure can't return a Result — ffmpeg progress is infallible by
    // design, matching `crate::ffmpeg::run`'s `FnMut(FfmpegProgress)`).
    let progress_err: RefCell<Option<AppError>> = RefCell::new(None);
    let convert_result = convert(
        source,
        opts,
        |fraction| {
            if progress_err.borrow().is_some() {
                return;
            }
            let res = (on_progress.borrow_mut())(Progress::FileProgress {
                source: source.to_path_buf(),
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

    let output = convert_result?;

    Ok(JobResult {
        output,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn opts(start_ms: u64, end_ms: u64) -> Opts {
        Opts { start_ms, end_ms }
    }

    /// Synthesize a tiny mp4 clip via the bundled ffmpeg so we have a real
    /// video on disk to trim. Same approach as the Video Format Converter
    /// tests — avoids committing binary fixtures.
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
    fn happy_path_writes_trimmed_clip_next_to_source() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 2);

        let events = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let result = run_job(&input, &opts(500, 1_500), &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("trim ok");

        let expected = dir.path().join("input_trimmed.mp4");
        assert_eq!(result.output, expected);
        assert!(expected.exists(), "expected output at {expected:?}");
        assert!(std::fs::metadata(&expected).unwrap().len() > 0);

        // First event is Started with the source path.
        let recorded = events.borrow();
        match &recorded[0] {
            Progress::Started { source } => assert_eq!(source, &input),
            other => panic!("expected Started first, got {other:?}"),
        }
    }

    #[test]
    fn end_beyond_duration_is_clamped_and_still_succeeds() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mp4", 1);
        let cancel = CancellationToken::new();
        // Ask for 0..60s of a 1s clip — end clamps to the source duration.
        let result =
            run_job(&input, &opts(0, 60_000), &cancel, |_| Ok(())).expect("clamped trim ok");
        assert!(result.output.exists());
    }

    #[test]
    fn invalid_range_returns_processing_failed_without_writing() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mp4", 1);
        let cancel = CancellationToken::new();
        // start >= end (after clamp) → rejected before any ffmpeg copy.
        let result = run_job(&input, &opts(800, 500), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        assert!(!dir.path().join("clip_trimmed.mp4").exists());
    }

    #[test]
    fn garbage_input_fails_and_leaves_no_partial_output() {
        let dir = TempDir::new().unwrap();
        let bad = dir.path().join("bad.mp4");
        std::fs::write(&bad, b"this is not an mp4 at all").unwrap();
        let cancel = CancellationToken::new();

        let result = run_job(&bad, &opts(0, 1_000), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        // convert() unlinks the partial on error.
        assert!(!dir.path().join("bad_trimmed.mp4").exists());
    }

    #[test]
    fn missing_input_is_returned_as_file_not_found() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope.mp4");
        let cancel = CancellationToken::new();
        let result = run_job(&missing, &opts(0, 1_000), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::FileNotFound { .. })));
    }

    #[test]
    fn cancel_before_any_work_returns_cancelled_with_no_events() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 1);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&input, &opts(0, 1_000), &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0, "no events fire pre-cancel");
        assert!(!dir.path().join("input_trimmed.mp4").exists());
    }

    #[test]
    fn cancel_from_started_callback_bails_before_writing() {
        // Cancel inside the Started emit. The post-Started cancel check
        // catches it before the ffmpeg spawn.
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 1);
        let cancel = CancellationToken::new();

        let result = run_job(&input, &opts(0, 1_000), &cancel, |p| {
            if matches!(p, Progress::Started { .. }) {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert!(!dir.path().join("input_trimmed.mp4").exists());
    }

    #[test]
    fn output_name_collision_routes_through_unique_path() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mp4", 1);
        let placeholder = dir.path().join("clip_trimmed.mp4");
        std::fs::write(&placeholder, b"placeholder").unwrap();
        let placeholder_bytes = std::fs::read(&placeholder).unwrap();

        let cancel = CancellationToken::new();
        let result =
            run_job(&input, &opts(0, 500), &cancel, |_| Ok(())).expect("trim ok despite collision");

        let resolved = dir.path().join("clip_trimmed (1).mp4");
        assert_eq!(result.output, resolved);
        assert!(resolved.exists());
        // Placeholder untouched.
        assert_eq!(std::fs::read(&placeholder).unwrap(), placeholder_bytes);
    }

    #[test]
    fn on_progress_error_aborts_the_job() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mp4", 1);
        let cancel = CancellationToken::new();
        let result = run_job(&input, &opts(0, 1_000), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }
}
