//! Audio Extractor orchestrator — 1 video input → N MP3 outputs (one per
//! audio track).
//!
//! Probes the source's audio stream count via [`crate::ffmpeg::probe_audio_stream_count`],
//! then iterates 0..track_count calling [`super::convert::extract_one_track`]
//! per track. No `Skipped` event variant — single-file shape means any
//! per-track failure aborts the whole job (with already-extracted prior
//! tracks left on disk, mirroring the Video Format Converter's
//! between-files cancellation rule).
//!
//! Cancellation semantics:
//! - Between tracks: checked at the top of each iteration → returns
//!   `AppError::Cancelled`. Already-written outputs stay on disk.
//! - Mid-track: [`crate::ffmpeg::run`] reaps the child and returns
//!   `AppError::Cancelled`, which propagates from `extract_one_track`. The
//!   in-flight partial `.mp3` is cleaned up by `extract_one_track`;
//!   already-written outputs from prior tracks stay on disk.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::extract_one_track;
use crate::error::{AppError, AppResult};
use crate::ffmpeg;

/// Per-track event streamed to the UI as the job progresses.
///
/// `index` is the 0-based track index (matches the ffmpeg `-map 0:a:<i>`
/// selector); `total` is the audio-track count discovered up front and
/// stays constant across the job. Each track fires `Started` first, then
/// zero or more `FileProgress`, then `Succeeded` — there is no `Skipped`
/// variant because any track failure aborts the whole job.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to extract this track. Emitted once per track.
    Started { index: u32, total: u32 },
    /// Mid-encode progress for the track currently in-flight. `fraction`
    /// is in `[0.0, 1.0]`. Emitted at most ~4× / sec (the ffmpeg shim
    /// throttles to 250 ms).
    FileProgress {
        index: u32,
        total: u32,
        fraction: f64,
    },
    /// Track extracted; output written at `output`.
    Succeeded {
        index: u32,
        total: u32,
        output: PathBuf,
    },
}

/// Result of a completed extraction job. `outputs` is in track order
/// (track 1 first); `outputs[0]` is the natural "Open output folder"
/// target on the UI side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct JobResult {
    pub track_count: u32,
    pub outputs: Vec<PathBuf>,
    pub duration_ms: u64,
}

/// Drive an Audio Extractor job end-to-end.
///
/// Errors:
/// - `AppError::FileNotFound` if `source` doesn't exist on disk.
/// - `AppError::ProcessingFailed { detail: "no audio streams" }` if the
///   source has no audio tracks.
/// - `AppError::Cancelled` if the token fires (between or mid-track).
/// - `AppError::ProcessingFailed` for any per-track ffmpeg failure
///   (aborts the job; prior successful outputs remain on disk).
/// - Any error returned by `on_progress` propagates unchanged.
pub fn run_job<F>(source: &Path, cancel: &CancellationToken, on_progress: F) -> AppResult<JobResult>
where
    F: FnMut(Progress) -> AppResult<()>,
{
    let start = Instant::now();

    if !source
        .try_exists()
        .map_err(|err| AppError::ProcessingFailed {
            detail: format!("stat {}: {err}", source.display()),
        })?
    {
        return Err(AppError::FileNotFound {
            path: source.display().to_string(),
        });
    }

    let track_count = ffmpeg::probe_audio_stream_count(source)?;
    if track_count == 0 {
        return Err(AppError::ProcessingFailed {
            detail: "no audio streams".into(),
        });
    }

    // RefCell so the inner per-track FileProgress closure (passed into
    // `extract_one_track`) and the outer orchestrator both call the
    // caller's emitter. Borrows are short-lived and non-reentrant.
    let on_progress = RefCell::new(on_progress);

    let mut outputs: Vec<PathBuf> = Vec::with_capacity(track_count as usize);

    for index in 0..track_count {
        if cancel.is_cancelled() {
            return Err(AppError::Cancelled);
        }

        (on_progress.borrow_mut())(Progress::Started {
            index,
            total: track_count,
        })?;

        // If a FileProgress emit fails, stop calling the emitter for this
        // track and propagate the captured error after `extract_one_track`
        // returns. We can't bubble it directly because the inner closure
        // must be `FnMut(f64)` — ffmpeg progress callbacks are infallible
        // by design (matches `crate::ffmpeg::run`'s `FnMut(FfmpegProgress)`).
        let progress_err: RefCell<Option<AppError>> = RefCell::new(None);
        let extract_result = extract_one_track(
            source,
            index,
            track_count,
            |fraction| {
                if progress_err.borrow().is_some() {
                    return;
                }
                let res = (on_progress.borrow_mut())(Progress::FileProgress {
                    index,
                    total: track_count,
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

        let output = extract_result?;
        outputs.push(output.clone());
        (on_progress.borrow_mut())(Progress::Succeeded {
            index,
            total: track_count,
            output,
        })?;
    }

    Ok(JobResult {
        track_count,
        outputs,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use tempfile::TempDir;

    /// Synthesize a tiny clip on disk via the bundled ffmpeg's `lavfi`
    /// source. `audio_tracks` controls how many sine generators get muxed
    /// in alongside the video — 0 yields a silent video, 1 yields the
    /// common single-track shape, 2+ yields the multi-track shape.
    ///
    /// Uses `.mkv` because mp4 multi-audio is supported but quirky in some
    /// ffmpeg builds; matroska accepts arbitrary stream layouts without
    /// fuss. The bundled ffmpeg is always available in any environment
    /// where these tests can run (build script puts it in `OUT_DIR`).
    fn synth_clip(dir: &Path, name: &str, audio_tracks: u32, duration: u32) -> PathBuf {
        let out = dir.join(name);
        let out_str = out.to_str().expect("utf-8 tempdir");

        let mut args: Vec<String> = Vec::new();
        // Video source (input #0).
        args.push("-f".into());
        args.push("lavfi".into());
        args.push("-i".into());
        args.push(format!("testsrc=duration={duration}:size=64x64:rate=10"));
        // Audio sources (inputs #1..=audio_tracks).
        for i in 0..audio_tracks {
            let freq = 440 + 200 * i;
            args.push("-f".into());
            args.push("lavfi".into());
            args.push("-i".into());
            args.push(format!("sine=frequency={freq}:duration={duration}"));
        }
        // Stream selection: map input #0's video, then each audio input.
        args.push("-map".into());
        args.push("0:v".into());
        for i in 1..=audio_tracks {
            args.push("-map".into());
            args.push(format!("{i}:a"));
        }
        // Codecs.
        args.push("-c:v".into());
        args.push("libx264".into());
        args.push("-preset".into());
        args.push("ultrafast".into());
        if audio_tracks > 0 {
            args.push("-c:a".into());
            args.push("aac".into());
        }
        args.push("-shortest".into());
        args.push(out_str.into());

        crate::ffmpeg::run(args, |_| {}, &CancellationToken::new())
            .expect("synthesize test clip via bundled ffmpeg");
        out
    }

    fn progress_kinds(events: &[Progress]) -> Vec<&'static str> {
        events
            .iter()
            .map(|p| match p {
                Progress::Started { .. } => "Started",
                Progress::FileProgress { .. } => "FileProgress",
                Progress::Succeeded { .. } => "Succeeded",
            })
            .collect()
    }

    #[test]
    fn single_track_happy_path_writes_unnumbered_output() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mkv", 1, 1);
        let cancel = CancellationToken::new();
        let events = RefCell::new(Vec::new());

        let result = run_job(&input, &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("single-track happy");

        assert_eq!(result.track_count, 1);
        assert_eq!(result.outputs.len(), 1);
        let expected = dir.path().join("clip_audio.mp3");
        assert_eq!(result.outputs[0], expected);
        assert!(expected.is_file());
        assert!(std::fs::metadata(&expected).unwrap().len() > 0);

        // No `_audio_1.mp3` (i.e. no track number) when there's only one.
        assert!(!dir.path().join("clip_audio_1.mp3").exists());

        let kinds = progress_kinds(&events.borrow());
        assert_eq!(kinds.first().copied(), Some("Started"));
        assert_eq!(kinds.last().copied(), Some("Succeeded"));
    }

    #[test]
    fn multi_track_happy_path_writes_one_numbered_output_per_track() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "concert.mkv", 3, 1);
        let cancel = CancellationToken::new();
        let events = RefCell::new(Vec::new());

        let result = run_job(&input, &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("multi-track happy");

        assert_eq!(result.track_count, 3);
        assert_eq!(result.outputs.len(), 3);
        assert_eq!(result.outputs[0], dir.path().join("concert_audio_1.mp3"));
        assert_eq!(result.outputs[1], dir.path().join("concert_audio_2.mp3"));
        assert_eq!(result.outputs[2], dir.path().join("concert_audio_3.mp3"));
        for out in &result.outputs {
            assert!(out.is_file(), "{} should exist", out.display());
            assert!(std::fs::metadata(out).unwrap().len() > 0);
        }

        // No `_audio.mp3` (unnumbered) when there's more than one track.
        assert!(!dir.path().join("concert_audio.mp3").exists());

        // Three `Started` and three `Succeeded` events, in track order.
        let started_indices: Vec<u32> = events
            .borrow()
            .iter()
            .filter_map(|p| match p {
                Progress::Started { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(started_indices, vec![0, 1, 2]);
        let succeeded_indices: Vec<u32> = events
            .borrow()
            .iter()
            .filter_map(|p| match p {
                Progress::Succeeded { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(succeeded_indices, vec![0, 1, 2]);
    }

    #[test]
    fn no_audio_source_returns_processing_failed_with_no_audio_streams_detail() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "silent.mkv", 0, 1);
        let cancel = CancellationToken::new();

        let result = run_job(&input, &cancel, |_| Ok(()));
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert_eq!(detail, "no audio streams");
            }
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }

        // No output should have been written.
        assert!(!dir.path().join("silent_audio.mp3").exists());
    }

    #[test]
    fn missing_input_returns_file_not_found() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope.mp4");
        let cancel = CancellationToken::new();

        let result = run_job(&missing, &cancel, |_| Ok(()));
        match result {
            Err(AppError::FileNotFound { path }) => {
                assert!(path.contains("nope.mp4"), "path was {path}");
            }
            other => panic!("expected FileNotFound, got {other:?}"),
        }
    }

    #[test]
    fn cancel_before_any_track_returns_cancelled_with_no_writes() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mkv", 1, 1);
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&input, &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0);
        assert!(!dir.path().join("clip_audio.mp3").exists());
    }

    #[test]
    fn cancel_mid_track_returns_cancelled_and_deletes_partial_output() {
        let dir = TempDir::new().unwrap();
        // 30s source so the encode runs long enough to cancel mid-stream.
        let input = synth_clip(dir.path(), "long.mkv", 1, 30);
        let cancel = CancellationToken::new();

        let result = run_job(&input, &cancel, |p| {
            // Cancel as soon as we see any mid-encode progress — proves we
            // genuinely interrupted ffmpeg, not just the pre-spawn check.
            if matches!(p, Progress::FileProgress { .. }) {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        // `extract_one_track` should have cleaned up the partial mp3.
        assert!(!dir.path().join("long_audio.mp3").exists());
    }

    #[test]
    fn cancel_between_tracks_preserves_already_written_outputs() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "concert.mkv", 2, 1);
        let cancel = CancellationToken::new();

        let result = run_job(&input, &cancel, |p| {
            if let Progress::Succeeded { index: 0, .. } = p {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        // First track written; second never attempted.
        assert!(dir.path().join("concert_audio_1.mp3").is_file());
        assert!(!dir.path().join("concert_audio_2.mp3").exists());
    }

    #[test]
    fn collision_routes_through_unique_path_and_leaves_existing_untouched() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mkv", 1, 1);
        let existing = dir.path().join("clip_audio.mp3");
        std::fs::write(&existing, b"keep me").unwrap();
        let cancel = CancellationToken::new();

        let result = run_job(&input, &cancel, |_| Ok(())).expect("collision-resolved write ok");

        let resolved = dir.path().join("clip_audio (1).mp3");
        assert_eq!(result.outputs[0], resolved);
        assert!(resolved.is_file());
        // Pre-existing file untouched.
        assert_eq!(std::fs::read(&existing).unwrap(), b"keep me");
    }

    #[test]
    fn on_progress_error_aborts_the_job() {
        let dir = TempDir::new().unwrap();
        let input = synth_clip(dir.path(), "clip.mkv", 1, 1);
        let cancel = CancellationToken::new();

        let result = run_job(&input, &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        match result {
            Err(AppError::ProcessingFailed { detail }) => assert_eq!(detail, "emit failed"),
            other => panic!("expected ProcessingFailed, got {other:?}"),
        }
    }
}
