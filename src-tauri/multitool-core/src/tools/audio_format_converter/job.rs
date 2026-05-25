//! Batch orchestrator for the Audio Format Converter.
//!
//! Mirrors `image_format_converter::job` in shape — same `Progress` /
//! `JobResult` wire types, same skip + continue rule, same cancellation
//! semantics.
//!
//! v1 limitation: cancellation is checked **between files only**. The
//! encoders all accept the full PCM buffer at once today (Symphonia gives
//! us the entire decoded buffer up front), so there's no natural
//! mid-file cancel checkpoint. For multi-minute audio files this can
//! feel laggy when cancelling mid-encode. The brief calls out a per-
//! chunk cancel as a follow-up; it requires switching the encoders to
//! streaming chunked I/O (LAME and Vorbis both support it; hound/flacenc
//! would need a per-frame loop).

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
    /// per-file notes (downmix/upmix, lossy-to-lossy transcode, etc.).
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
    /// Full path of the first successful output file (handy for "reveal
    /// in folder"). `None` when no file succeeded.
    pub first_output_path: Option<PathBuf>,
    pub duration_ms: u64,
}

/// Run the full batch end-to-end. Returns once every file has been
/// processed or until cancellation triggers between files.
///
/// Per-file failures are accumulated into [`JobResult::skipped`]; the
/// job itself returns `Ok(JobResult)` unless one of the orchestrator-
/// level aborts fires:
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
            detail: "no audio files to convert".into(),
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
/// **before** `unique_path` resolution.
fn derive_output_path(source: &Path, target_format: TargetFormat) -> PathBuf {
    let mut out = source.to_path_buf();
    out.set_extension(target_format.extension());
    out
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
    use super::super::convert::ChannelMode;
    use super::*;
    use crate::audio_codecs::encode::WavBitDepth;
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn audio_fixture_path(name: &str) -> PathBuf {
        PathBuf::from(format!("tests/fixtures/audio/{name}"))
    }

    fn default_opts(target_format: TargetFormat) -> Opts {
        Opts {
            target_format,
            mp3_bitrate_kbps: 192,
            vorbis_quality: 5.0,
            flac_compression_level: 5,
            wav_bit_depth: WavBitDepth::Bit16,
            channels: ChannelMode::Source,
        }
    }

    fn copy_fixture_to(dir: &Path, name: &str) -> PathBuf {
        let dst = dir.join(name);
        fs::copy(audio_fixture_path(name), &dst).expect("copy fixture");
        dst
    }

    #[test]
    fn happy_path_three_file_batch_writes_outputs_and_reports_counts() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![
            copy_fixture_to(dir.path(), "tiny_mono.wav"),
            copy_fixture_to(dir.path(), "tiny_mono.mp3"),
            copy_fixture_to(dir.path(), "tiny_mono.flac"),
        ];

        let events = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &default_opts(TargetFormat::Wav), &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("run_job ok");

        assert_eq!(result.success_count, 3);
        assert_eq!(result.skip_count, 0);
        let first = result.first_output_path.as_deref().expect("first output");
        assert_eq!(first.parent(), Some(dir.path()));
        assert!(first.exists(), "{first:?} should exist");

        // 3 inputs × 2 events (Started + Succeeded) = 6 events.
        assert_eq!(events.borrow().len(), 6);
    }

    #[test]
    fn mid_batch_garbage_file_is_skipped_and_others_succeed() {
        let dir = TempDir::new().expect("tempdir");
        let bad_path = dir.path().join("garbage.wav");
        fs::write(&bad_path, b"this is not a WAV file at all").expect("write garbage");
        let inputs = vec![
            copy_fixture_to(dir.path(), "tiny_mono.wav"),
            bad_path.clone(),
            copy_fixture_to(dir.path(), "tiny_stereo.wav"),
        ];

        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &default_opts(TargetFormat::Flac), &cancel, |_| {
            Ok(())
        })
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
        let missing = dir.path().join("does_not_exist.wav");
        let inputs = vec![missing.clone()];

        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &default_opts(TargetFormat::Wav), &cancel, |_| {
            Ok(())
        })
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
        let inputs = vec![copy_fixture_to(dir.path(), "tiny_mono.wav")];
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&inputs, &default_opts(TargetFormat::Wav), &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0, "no progress events fire pre-cancel");
        assert!(!dir.path().join("tiny_mono (1).wav").exists());
    }

    #[test]
    fn cancel_between_files_preserves_already_written_outputs() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![
            copy_fixture_to(dir.path(), "tiny_mono.wav"),
            copy_fixture_to(dir.path(), "tiny_stereo.wav"),
        ];
        let cancel = CancellationToken::new();

        let result = run_job(
            &inputs,
            &default_opts(TargetFormat::Flac),
            &cancel,
            |progress| {
                if let Progress::Succeeded { index: 0, .. } = progress {
                    cancel.cancel();
                }
                Ok(())
            },
        );
        assert!(matches!(result, Err(AppError::Cancelled)));
        // First file's FLAC should exist; second should not.
        assert!(dir.path().join("tiny_mono.flac").exists());
        assert!(!dir.path().join("tiny_stereo.flac").exists());
    }

    #[test]
    fn output_name_collision_routes_through_unique_path() {
        let dir = TempDir::new().expect("tempdir");
        // Same-format request: copy tiny_mono.wav, convert to WAV at the
        // same location → collision → output should be "tiny_mono (1).wav".
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");
        let original_bytes = fs::read(&input).unwrap();

        let cancel = CancellationToken::new();
        let result = run_job(
            std::slice::from_ref(&input),
            &default_opts(TargetFormat::Wav),
            &cancel,
            |_| Ok(()),
        )
        .expect("same-format conversion ok");

        assert_eq!(result.success_count, 1);
        let new_path = dir.path().join("tiny_mono (1).wav");
        assert!(
            new_path.exists(),
            "expected tiny_mono (1).wav at {new_path:?}"
        );
        // Original untouched.
        assert_eq!(fs::read(&input).unwrap(), original_bytes);
    }

    #[test]
    fn on_progress_error_aborts_the_job() {
        let dir = TempDir::new().expect("tempdir");
        let inputs = vec![copy_fixture_to(dir.path(), "tiny_mono.wav")];
        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &default_opts(TargetFormat::Wav), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }

    #[test]
    fn empty_inputs_yield_processing_failed() {
        let cancel = CancellationToken::new();
        let result = run_job(&[], &default_opts(TargetFormat::Wav), &cancel, |_| Ok(()));
        match result {
            Err(AppError::ProcessingFailed { detail }) => {
                assert_eq!(detail, "no audio files to convert");
            }
            other => panic!("expected ProcessingFailed for empty inputs, got {other:?}"),
        }
    }

    #[test]
    fn downmix_warning_lands_in_succeeded_event() {
        let dir = TempDir::new().expect("tempdir");
        // Force ChannelMode::Mono so the stereo fixture downmixes (and
        // emits a warning) on its way to WAV output.
        let inputs = vec![copy_fixture_to(dir.path(), "tiny_stereo.wav")];
        let mut opts = default_opts(TargetFormat::Wav);
        opts.channels = ChannelMode::Mono;

        let events = RefCell::new(Vec::new());
        let cancel = CancellationToken::new();
        let result = run_job(&inputs, &opts, &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("run_job ok");
        assert_eq!(result.success_count, 1);

        let succeeded_warnings: Vec<_> = events
            .borrow()
            .iter()
            .filter_map(|e| match e {
                Progress::Succeeded { warnings, .. } => Some(warnings.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(succeeded_warnings.len(), 1);
        assert!(
            succeeded_warnings[0].iter().any(|w| w.contains("mono")),
            "expected mono-downmix warning in Succeeded event, got {:?}",
            succeeded_warnings[0]
        );
    }
}
