//! Video Compress — single-file orchestrator.
//!
//! Mirrors [`super::super::video_trimmer::job`] (one file in, one file
//! out, no skip-and-continue lane) but drives [`super::convert`]
//! instead. The single mid-run `FileProgress` variant streams the
//! re-encode's 0..=1 fraction sourced from `out_time_us / duration`.
//!
//! Cancellation:
//! - Before any work and again right after the `Started` emit (before
//!   the ffmpeg spawn) → returns `AppError::Cancelled`, nothing
//!   written.
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

/// Per-file event streamed to the UI as the compress progresses.
///
/// `Started` fires once when the re-encode begins; zero or more
/// `FileProgress` follow with the mid-encode 0..=1 fraction (throttled
/// to ~4/sec by the ffmpeg shim). On success the job resolves with
/// [`JobResult`]; on failure it rejects with an `AppError`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    Started { source: PathBuf },
    FileProgress { source: PathBuf, fraction: f64 },
}

/// Result of a completed compress.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    /// Path the compressed clip was written to. Routes through
    /// [`crate::fs::unique_path`], so a same-name collision lands at
    /// `{stem}_compressed (1).{ext}`.
    pub output: PathBuf,
    pub duration_ms: u64,
}

/// Run the compress end-to-end. Returns once the compressed clip is
/// written, or earlier with an `AppError` if anything fails.
///
/// Aborts:
/// - cancellation (before/after `Started`, or mid-encode) → `Cancelled`
/// - the caller's `on_progress` returning `Err(...)` → propagated
///   unchanged
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
    // orchestrator both reach the caller's fallible emitter. Same
    // shape as the trimmer's `run_job`.
    let on_progress = RefCell::new(on_progress);

    (on_progress.borrow_mut())(Progress::Started {
        source: source.to_path_buf(),
    })?;

    // Cancel between Started and the ffmpeg spawn keeps the cancel
    // path deterministic — a compress of a sub-second clip can
    // otherwise finish before the shim's read loop notices the token.
    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    // If a FileProgress emit fails, stash the error and stop emitting
    // for the rest of the run; propagate it after `convert` returns
    // (the inner closure can't return a Result — ffmpeg progress is
    // infallible by design, matching `crate::ffmpeg::run`'s
    // `FnMut(FfmpegProgress)`).
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

    fn opts(quality: f64) -> Opts {
        Opts { quality }
    }

    /// Synthesize a tiny mp4 clip via the bundled ffmpeg so we have a
    /// real video on disk to compress. Same approach as the trimmer's
    /// tests.
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

    fn synth_silent_clip(dir: &Path, name: &str, duration: u32) -> PathBuf {
        let out = dir.join(name);
        let out_str = out.to_str().expect("utf-8 tempdir");
        let args = [
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=duration={duration}:size=64x64:rate=10"),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            out_str,
        ];
        crate::ffmpeg::run(args, |_| {}, &CancellationToken::new())
            .expect("synthesize silent test clip");
        out
    }

    #[test]
    fn happy_path_writes_compressed_clip_next_to_source() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 1);

        let events = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let result = run_job(&input, &opts(0.5), &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("compress ok");

        let expected = dir.path().join("input_compressed.mp4");
        assert_eq!(result.output, expected);
        assert!(expected.exists(), "expected output at {expected:?}");
        assert!(std::fs::metadata(&expected).unwrap().len() > 0);

        let recorded = events.borrow();
        match &recorded[0] {
            Progress::Started { source } => assert_eq!(source, &input),
            other => panic!("expected Started first, got {other:?}"),
        }
    }

    #[test]
    fn silent_video_compresses_cleanly_without_audio_track() {
        let dir = TempDir::new().unwrap();
        let input = synth_silent_clip(dir.path(), "silent.mp4", 1);
        let cancel = CancellationToken::new();
        let result = run_job(&input, &opts(0.5), &cancel, |_| Ok(())).expect("compress silent ok");
        assert!(result.output.exists());
    }

    #[test]
    fn garbage_input_fails_and_leaves_no_partial_output() {
        let dir = TempDir::new().unwrap();
        let bad = dir.path().join("bad.mp4");
        std::fs::write(&bad, b"this is not an mp4 at all").unwrap();
        let cancel = CancellationToken::new();

        let result = run_job(&bad, &opts(0.5), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        assert!(!dir.path().join("bad_compressed.mp4").exists());
    }

    #[test]
    fn missing_input_is_returned_as_file_not_found() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope.mp4");
        let cancel = CancellationToken::new();
        let result = run_job(&missing, &opts(0.5), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::FileNotFound { .. })));
    }

    #[test]
    fn cancel_before_any_work_returns_cancelled_with_no_events() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 1);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&input, &opts(0.5), &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0, "no events fire pre-cancel");
        assert!(!dir.path().join("input_compressed.mp4").exists());
    }

    #[test]
    fn cancel_from_started_callback_bails_before_writing() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "input.mp4", 1);
        let cancel = CancellationToken::new();

        let result = run_job(&input, &opts(0.5), &cancel, |p| {
            if matches!(p, Progress::Started { .. }) {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert!(!dir.path().join("input_compressed.mp4").exists());
    }

    #[test]
    fn output_name_collision_routes_through_unique_path() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mp4", 1);
        let placeholder = dir.path().join("clip_compressed.mp4");
        std::fs::write(&placeholder, b"placeholder").unwrap();
        let placeholder_bytes = std::fs::read(&placeholder).unwrap();

        let cancel = CancellationToken::new();
        let result = run_job(&input, &opts(0.5), &cancel, |_| Ok(()))
            .expect("compress ok despite collision");

        let resolved = dir.path().join("clip_compressed (1).mp4");
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
        let result = run_job(&input, &opts(0.5), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }
}
