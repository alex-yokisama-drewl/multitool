//! Video Trimmer — single-file pure pipeline.
//!
//! `(source, opts)` in, output `PathBuf` out. Trimming is an ffmpeg
//! **stream copy** (`-ss`/`-t` + `-c copy`): near-instant, lossless, and
//! works for any container ffmpeg can demux+remux. ffmpeg-specific
//! spawning lives in [`crate::ffmpeg`]; this module owns the trimmer's
//! wire types, the arg-vec builder, and the output-naming rule.
//!
//! Cut precision: `-ss` before `-i` is an input seek that snaps to the
//! keyframe at or before the requested start, so the real output start
//! can land slightly earlier than asked (up to one GOP). That's the
//! accepted tradeoff for a copy-based trim — frame-accurate trimming
//! would require re-encoding, which is a separate tool's story.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::ffmpeg;
use crate::fs::unique_path;

/// User-facing trim options. Range bounds are milliseconds from the start
/// of the source.
///
/// `end_ms` is clamped to the probed source duration in [`convert`];
/// `start_ms >= end_ms` (after clamping) is rejected with
/// `ProcessingFailed`. No fades, no codec knobs — the output is a stream
/// copy of the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Opts {
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Trim a single video file to `[start_ms, end_ms]`. Probes the source's
/// duration (to clamp `end_ms` and drive the 0..=1 progress fraction),
/// builds the ffmpeg arg vec, and drives [`crate::ffmpeg::run`] under the
/// supplied cancellation token.
///
/// Returns the collision-resolved output path on success.
///
/// On any error (including cancellation) the partial output file is
/// deleted before returning — a half-written clip is just garbage.
///
/// Errors:
/// - `AppError::FileNotFound` if `source` does not exist on disk.
/// - `AppError::ProcessingFailed` if the range is empty after clamping,
///   if ffmpeg can't probe a duration, or if the encode exits non-zero.
/// - `AppError::Cancelled` if the token fires mid-copy.
pub fn convert<F>(
    source: &Path,
    opts: &Opts,
    mut on_progress: F,
    cancel: &CancellationToken,
) -> AppResult<PathBuf>
where
    F: FnMut(f64),
{
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

    // Duration both clamps `end_ms` and feeds the progress fraction. If
    // ffmpeg can't read the input enough to report a Duration line, the
    // copy would have failed anyway — surface that as the error.
    let duration_secs = ffmpeg::probe_duration_secs(source)?;
    let duration_ms = (duration_secs * 1000.0).round().max(0.0) as u64;
    let end_ms = opts.end_ms.min(duration_ms);

    if opts.start_ms >= end_ms {
        return Err(AppError::ProcessingFailed {
            detail: format!(
                "video trim: invalid range (start_ms={start} >= end_ms={end}, source duration {dur} ms)",
                start = opts.start_ms,
                end = end_ms,
                dur = duration_ms,
            ),
        });
    }
    let trim_ms = end_ms - opts.start_ms;

    let target_path = derive_output_path(source);
    let final_path = unique_path(&target_path).map_err(|err| AppError::ProcessingFailed {
        detail: format!("derive unique output {}: {err}", target_path.display()),
    })?;

    let args = build_args(source, &final_path, opts.start_ms, trim_ms);
    let trim_secs = (trim_ms as f64) / 1000.0;
    let result = ffmpeg::run(
        &args,
        |p| {
            let fraction = if trim_secs > 0.0 {
                ((p.out_time_us as f64) / 1_000_000.0 / trim_secs).clamp(0.0, 1.0)
            } else {
                0.0
            };
            on_progress(fraction);
        },
        cancel,
    );

    match result {
        Ok(()) => Ok(final_path),
        Err(err) => {
            // Best-effort cleanup. If ffmpeg failed before opening the
            // output, `remove_file` returns NotFound which we ignore.
            let _ = std::fs::remove_file(&final_path);
            Err(err)
        }
    }
}

/// `{stem}_trimmed.{ext}` next to the source, **before** `unique_path`
/// resolution. Mirrors the Audio Trimmer's naming. A missing extension
/// (picker filter should make this unreachable) falls back to a bare
/// `{stem}_trimmed`.
pub(super) fn derive_output_path(source: &Path) -> PathBuf {
    let parent = source.parent();
    let mut name = source.file_stem().map(|s| s.to_owned()).unwrap_or_default();
    name.push("_trimmed");
    if let Some(ext) = source.extension() {
        name.push(".");
        name.push(ext);
    }
    match parent {
        Some(p) => p.join(name),
        None => PathBuf::from(name),
    }
}

/// Build the ffmpeg arg vec for one trim. `-ss` before `-i` is a fast
/// input seek (keyframe-snapped); `-t <dur>` after `-i` bounds the output
/// length unambiguously (sidestepping the `-to` relative/absolute
/// ambiguity after an input seek). `-map 0:v? -map 0:a?` copies all
/// video and audio streams (optional, so a video-only or audio-less
/// source still works) while dropping subtitle/data/attachment streams —
/// copying those across containers is a footgun. `-avoid_negative_ts
/// make_zero` rebases timestamps so the cut output starts at 0. `-y`
/// keeps ffmpeg from going interactive if a residue file somehow exists
/// (it shouldn't — `unique_path` ran).
fn build_args(source: &Path, output: &Path, start_ms: u64, trim_ms: u64) -> Vec<OsString> {
    vec![
        "-y".into(),
        "-ss".into(),
        fmt_secs(start_ms).into(),
        "-i".into(),
        source.as_os_str().to_os_string(),
        "-t".into(),
        fmt_secs(trim_ms).into(),
        "-map".into(),
        "0:v?".into(),
        "-map".into(),
        "0:a?".into(),
        "-c".into(),
        "copy".into(),
        "-avoid_negative_ts".into(),
        "make_zero".into(),
        output.as_os_str().to_os_string(),
    ]
}

/// Format a millisecond count as `S.mmm` seconds for ffmpeg's `-ss` / `-t`
/// (e.g. `90250` → `"90.250"`). ffmpeg parses fractional-seconds durations
/// directly, so we never need the `HH:MM:SS` form.
fn fmt_secs(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_secs_formats_fractional_seconds() {
        assert_eq!(fmt_secs(0), "0.000");
        assert_eq!(fmt_secs(250), "0.250");
        assert_eq!(fmt_secs(1_000), "1.000");
        assert_eq!(fmt_secs(90_250), "90.250");
    }

    #[test]
    fn output_path_appends_trimmed_before_extension() {
        let src = PathBuf::from("/videos/holiday.mp4");
        let out = derive_output_path(&src);
        assert_eq!(out, PathBuf::from("/videos/holiday_trimmed.mp4"));
    }

    #[test]
    fn output_path_handles_multi_dot_stem() {
        // file_stem keeps everything before the LAST dot.
        let src = PathBuf::from("/videos/report.final.mov");
        let out = derive_output_path(&src);
        assert_eq!(out, PathBuf::from("/videos/report.final_trimmed.mov"));
    }

    #[test]
    fn output_path_without_extension_falls_back_to_bare_stem() {
        let src = PathBuf::from("/videos/clip");
        let out = derive_output_path(&src);
        assert_eq!(out, PathBuf::from("/videos/clip_trimmed"));
    }

    #[test]
    fn build_args_input_seek_before_input_then_duration_after() {
        let args = build_args(Path::new("/in.mkv"), Path::new("/out.mkv"), 1_500, 4_000);
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(joined[0], "-y");
        // -ss must precede -i (fast input seek).
        let ss_idx = joined.iter().position(|&s| s == "-ss").unwrap();
        let i_idx = joined.iter().position(|&s| s == "-i").unwrap();
        assert!(ss_idx < i_idx, "-ss must come before -i");
        assert_eq!(joined[ss_idx + 1], "1.500");
        assert_eq!(joined[i_idx + 1], "/in.mkv");
        // -t (duration) must come AFTER -i.
        let t_idx = joined.iter().position(|&s| s == "-t").unwrap();
        assert!(t_idx > i_idx, "-t must come after -i");
        assert_eq!(joined[t_idx + 1], "4.000");
        assert_eq!(joined.last().copied(), Some("/out.mkv"));
    }

    #[test]
    fn build_args_copy_av_streams_only_and_rebase_timestamps() {
        let args = build_args(Path::new("/in.mp4"), Path::new("/out.mp4"), 0, 1_000);
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        // Stream copy, not re-encode.
        let c_idx = joined.iter().position(|&s| s == "-c").unwrap();
        assert_eq!(joined[c_idx + 1], "copy");
        // Optional video + audio maps, no subtitle/data map.
        assert!(joined.windows(2).any(|w| w == ["-map", "0:v?"]));
        assert!(joined.windows(2).any(|w| w == ["-map", "0:a?"]));
        assert!(!joined.iter().any(|&s| s == "0:s" || s == "0:s?"));
        // Timestamp rebase so the cut starts at 0.
        let ats_idx = joined
            .iter()
            .position(|&s| s == "-avoid_negative_ts")
            .unwrap();
        assert_eq!(joined[ats_idx + 1], "make_zero");
        // No CRF / bitrate args — a copy doesn't take them.
        assert!(!joined.iter().any(|&s| s == "-crf" || s == "-b:v"));
    }
}
