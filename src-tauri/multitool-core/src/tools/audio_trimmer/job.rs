//! Audio Trimmer — single-file orchestrator.
//!
//! Pipeline: read source bytes → decode → cancel check → trim + fade →
//! cancel check → encode in the source format → write to
//! `{stem}_trimmed.{ext}` (or the next free `(n)` suffix).
//!
//! Cancellation is checked at two natural seams: between decode and
//! trim, and between trim and encode. Mid-encode cancel inherits the
//! same v1 limitation as the Audio Format Converter (encoders take the
//! full PCM buffer at once); see DECISIONS.md → "Audio stack".
//!
//! Output format = source format. The trimmer's contract is "preserves
//! source format"; supported extensions are wav / mp3 / flac / ogg / oga
//! (the four formats we have encoders for; DECISIONS.md → "Audio
//! Trimmer: source-format-preserving" follow-up will codify this). Any
//! other extension lands as `AppError::UnsupportedFormat` from
//! [`encode_for_source`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::convert::{trim_and_fade, Opts};
use crate::audio_codecs::decode::decode_to_pcm;
use crate::audio_codecs::encode::{
    encode_flac, encode_mp3, encode_ogg_vorbis, encode_wav, validate_mp3_sample_rate, WavBitDepth,
};
use crate::audio_codecs::AudioBuffer;
use crate::error::{AppError, AppResult};
use crate::fs::unique_path;

/// Default WAV bit depth when re-encoding a trimmed WAV. 16-bit covers
/// the typical "tiny audio file" v1 use case; reading the source's
/// actual bit depth and matching it is a follow-up.
const WAV_DEFAULT_BIT_DEPTH: WavBitDepth = WavBitDepth::Bit16;
/// Default MP3 CBR bitrate when re-encoding a trimmed MP3.
const MP3_DEFAULT_BITRATE_KBPS: u32 = 192;
/// Default OGG Vorbis quality (Xiph CLI scale) when re-encoding.
const OGG_DEFAULT_QUALITY: f32 = 5.0;
/// FLAC compression level is a no-op in v1 (the v1 flacenc default is
/// used regardless); passing any value through for forward-compat.
const FLAC_DEFAULT_COMPRESSION: u32 = 5;

/// Per-file event streamed to the UI as the job progresses.
///
/// Single variant in v1: the trimmer fires `Started` once when decode
/// begins, then either resolves with [`JobResult`] (success) or rejects
/// with an `AppError` (failure). No skip-and-continue path — there's
/// only one file.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    /// About to decode + trim + encode the picked file.
    Started { source: PathBuf },
}

/// Result of a completed trim.
#[derive(Clone, Debug, Serialize)]
pub struct JobResult {
    /// The path the trimmed bytes were written to. Routes through
    /// `multitool_core::fs::unique_path` so a same-name collision lands
    /// at `{stem}_trimmed (1).{ext}`.
    pub output: PathBuf,
    /// Per-file notes (e.g. overlap-clamp from [`trim_and_fade`]).
    /// Empty on the happy path.
    pub warnings: Vec<String>,
    pub duration_ms: u64,
}

/// Run the trim job end-to-end. Returns once the trimmed bytes are
/// written, or earlier with an `AppError` if anything fails.
pub fn run_job<F>(
    source: &Path,
    opts: &Opts,
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

    let decoded = decode_to_pcm(&source_ext, &bytes)?;

    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    let (trimmed, warnings) = trim_and_fade(
        &decoded,
        opts.start_ms,
        opts.end_ms,
        opts.fade_in_ms,
        opts.fade_out_ms,
    )?;

    if cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    let encoded = encode_for_source(&source_ext, &trimmed)?;

    let target_path = derive_output_path(source);
    let final_path = unique_path(&target_path).map_err(|err| io_to_app_err(&target_path, &err))?;
    fs::write(&final_path, &encoded).map_err(|err| io_to_app_err(&final_path, &err))?;

    Ok(JobResult {
        output: final_path,
        warnings,
        duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// Route the trimmed buffer into the matching encoder. The source's
/// extension picks the lane; per-format defaults (WAV bit depth, MP3
/// bitrate, OGG quality) come from this file's `*_DEFAULT_*` consts —
/// the trimmer's contract doesn't expose them on `Opts`, on purpose
/// (output preserves source format; for finer control the user pipes
/// the trim output through the Audio Format Converter).
fn encode_for_source(source_ext: &str, buf: &AudioBuffer) -> AppResult<Vec<u8>> {
    match source_ext {
        "wav" => encode_wav(buf, WAV_DEFAULT_BIT_DEPTH),
        "flac" => encode_flac(buf, FLAC_DEFAULT_COMPRESSION),
        "mp3" => {
            validate_mp3_sample_rate(buf.sample_rate)?;
            encode_mp3(buf, MP3_DEFAULT_BITRATE_KBPS)
        }
        "ogg" | "oga" => encode_ogg_vorbis(buf, OGG_DEFAULT_QUALITY),
        other => Err(AppError::UnsupportedFormat {
            detail: format!(
                "audio trim: source extension '{other}' has no encoder (supported: wav/mp3/flac/ogg/oga)"
            ),
        }),
    }
}

/// Compute the desired output path, **before** `unique_path` resolution.
///
/// `song.mp3` → `song_trimmed.mp3`. Preserves the source directory.
/// Strips a missing-extension source by falling back to a `_trimmed`
/// suffix on the bare file name — the picker filter should make that
/// path unreachable, but the defensive branch keeps the orchestrator
/// honest.
fn derive_output_path(source: &Path) -> PathBuf {
    let parent = source.parent();
    let stem = source.file_stem().map(|s| s.to_owned()).unwrap_or_default();
    let mut new_stem = stem;
    new_stem.push("_trimmed");

    let mut name = new_stem;
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

    fn audio_fixture_path(name: &str) -> PathBuf {
        PathBuf::from(format!("tests/fixtures/audio/{name}"))
    }

    fn copy_fixture_to(dir: &Path, name: &str) -> PathBuf {
        let dst = dir.join(name);
        fs::copy(audio_fixture_path(name), &dst).expect("copy fixture");
        dst
    }

    fn default_opts() -> Opts {
        Opts {
            start_ms: 50,
            end_ms: 200,
            fade_in_ms: 0,
            fade_out_ms: 0,
        }
    }

    #[test]
    fn happy_path_writes_trimmed_wav_alongside_source_with_expected_name() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");

        let cancel = CancellationToken::new();
        let events = RefCell::new(Vec::new());
        let result = run_job(&input, &default_opts(), &cancel, |p| {
            events.borrow_mut().push(p);
            Ok(())
        })
        .expect("trim ok");

        // Naming: `{stem}_trimmed.{ext}` next to the source.
        let expected = dir.path().join("tiny_mono_trimmed.wav");
        assert_eq!(result.output, expected);
        assert!(expected.exists(), "expected output at {expected:?}");

        // No overlap → no warnings.
        assert!(result.warnings.is_empty());

        // One Progress::Started event with the source path.
        assert_eq!(events.borrow().len(), 1);
        match &events.borrow()[0] {
            Progress::Started { source } => assert_eq!(source, &input),
        }

        // Output is decodable + dim-preserving (mono / 44.1 kHz).
        let bytes = fs::read(&expected).expect("read trimmed");
        let decoded = decode_to_pcm("wav", &bytes).expect("re-decode trimmed wav");
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.sample_rate, 44_100);
        // 150 ms × 44_100 Hz = 6_615 frames, mono → 6_615 samples.
        assert_eq!(decoded.samples.len(), 6_615);
    }

    #[test]
    fn happy_path_mp3_source_round_trips_to_mp3_output() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.mp3");

        let cancel = CancellationToken::new();
        let result = run_job(&input, &default_opts(), &cancel, |_| Ok(())).expect("trim ok");

        let expected = dir.path().join("tiny_mono_trimmed.mp3");
        assert_eq!(result.output, expected);
        assert!(expected.exists());

        // Output is decodable as MP3.
        let bytes = fs::read(&expected).expect("read trimmed mp3");
        let decoded = decode_to_pcm("mp3", &bytes).expect("re-decode trimmed mp3");
        assert_eq!(decoded.sample_rate, 44_100);
    }

    #[test]
    fn happy_path_flac_source_round_trips_to_flac_output() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.flac");

        let cancel = CancellationToken::new();
        let result = run_job(&input, &default_opts(), &cancel, |_| Ok(())).expect("trim ok");

        let expected = dir.path().join("tiny_mono_trimmed.flac");
        assert!(expected.exists(), "expected {expected:?}");
        assert_eq!(result.output, expected);
    }

    #[test]
    fn happy_path_ogg_source_round_trips_to_ogg_output() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.ogg");

        let cancel = CancellationToken::new();
        let result = run_job(&input, &default_opts(), &cancel, |_| Ok(())).expect("trim ok");

        let expected = dir.path().join("tiny_mono_trimmed.ogg");
        assert!(expected.exists());
        assert_eq!(result.output, expected);
    }

    #[test]
    fn invalid_range_returns_processing_failed_without_writing_output() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");

        let cancel = CancellationToken::new();
        let mut bad = default_opts();
        bad.start_ms = 200;
        bad.end_ms = 100;
        let result = run_job(&input, &bad, &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        // Nothing should have been written.
        assert!(!dir.path().join("tiny_mono_trimmed.wav").exists());
    }

    #[test]
    fn missing_input_file_is_returned_as_file_not_found() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("does_not_exist.wav");

        let cancel = CancellationToken::new();
        let result = run_job(&missing, &default_opts(), &cancel, |_| Ok(()));
        assert!(matches!(result, Err(AppError::FileNotFound { .. })));
    }

    #[test]
    fn unsupported_source_extension_is_rejected_with_unsupported_format() {
        // .wav decodes fine but if we feed a file with a `.bogus`
        // extension the encoder lookup rejects it. (The picker's filter
        // shouldn't let this happen — defensive coverage for direct
        // IPC callers.)
        let dir = TempDir::new().expect("tempdir");
        let src = audio_fixture_path("tiny_mono.wav");
        let bogus = dir.path().join("audio.bogus");
        fs::copy(&src, &bogus).expect("copy fixture");

        let cancel = CancellationToken::new();
        let result = run_job(&bogus, &default_opts(), &cancel, |_| Ok(()));
        // Symphonia fails to decode without a known extension/magic
        // (the wav magic IS sniffed, so this actually decodes). The
        // encoder lookup is what rejects.
        match result {
            Err(AppError::UnsupportedFormat { detail }) => {
                assert!(detail.contains("bogus"), "got: {detail}");
            }
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    #[test]
    fn cancellation_before_decode_returns_cancelled_with_no_writes() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");
        let cancel = CancellationToken::new();
        cancel.cancel();

        let calls = RefCell::new(0usize);
        let result = run_job(&input, &default_opts(), &cancel, |_| {
            *calls.borrow_mut() += 1;
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert_eq!(*calls.borrow(), 0, "no progress events fire pre-cancel");
        assert!(!dir.path().join("tiny_mono_trimmed.wav").exists());
    }

    #[test]
    fn cancellation_after_started_progress_returns_cancelled_without_writing() {
        // Trigger cancel from inside the Progress::Started callback.
        // The second cancel-check (post-decode) catches it and bails
        // before encode / write.
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");
        let cancel = CancellationToken::new();

        let result = run_job(&input, &default_opts(), &cancel, |progress| {
            if matches!(progress, Progress::Started { .. }) {
                cancel.cancel();
            }
            Ok(())
        });
        assert!(matches!(result, Err(AppError::Cancelled)));
        assert!(!dir.path().join("tiny_mono_trimmed.wav").exists());
    }

    #[test]
    fn output_name_collision_routes_through_unique_path() {
        // Pre-create `tiny_mono_trimmed.wav` so the orchestrator must
        // resolve to `tiny_mono_trimmed (1).wav`.
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");
        let placeholder = dir.path().join("tiny_mono_trimmed.wav");
        fs::write(&placeholder, b"placeholder").expect("write placeholder");
        let placeholder_bytes = fs::read(&placeholder).expect("read placeholder");

        let cancel = CancellationToken::new();
        let result = run_job(&input, &default_opts(), &cancel, |_| Ok(()))
            .expect("trim ok despite collision");

        let new_path = dir.path().join("tiny_mono_trimmed (1).wav");
        assert!(new_path.exists(), "expected {new_path:?}");
        assert_eq!(result.output, new_path);
        // The placeholder must be untouched.
        assert_eq!(fs::read(&placeholder).unwrap(), placeholder_bytes);
    }

    #[test]
    fn warnings_from_trim_and_fade_ride_along_on_the_job_result() {
        // The mono fixture is 0.25 s = 250 ms. Trim a 100 ms window and
        // ask for fades that overlap → trim_and_fade emits a warning.
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");
        let cancel = CancellationToken::new();

        let opts = Opts {
            start_ms: 50,
            end_ms: 150, // 100 ms window
            fade_in_ms: 70,
            fade_out_ms: 50, // sum = 120 > 100 → overlap clamp
        };
        let result = run_job(&input, &opts, &cancel, |_| Ok(())).expect("trim ok");
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0].contains("clamped"),
            "got: {}",
            result.warnings[0]
        );
    }

    #[test]
    fn on_progress_error_aborts_the_job_before_decode() {
        let dir = TempDir::new().expect("tempdir");
        let input = copy_fixture_to(dir.path(), "tiny_mono.wav");
        let cancel = CancellationToken::new();
        let result = run_job(&input, &default_opts(), &cancel, |_| {
            Err(AppError::ProcessingFailed {
                detail: "emit failed".into(),
            })
        });
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        assert!(!dir.path().join("tiny_mono_trimmed.wav").exists());
    }
}
