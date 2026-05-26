//! Video Format Converter — single-file pure pipeline.
//!
//! `(source, opts)` in, output `PathBuf` out. The orchestrator in
//! [`super::job`] drives this in a per-file loop. ffmpeg-specific spawning
//! lives in [`crate::ffmpeg`]; this module owns the converter's wire types
//! and the codec-recipe table.
//!
//! The recipe per target is **baked in** for v1 — the UI exposes only the
//! target-format dropdown, not bitrate / preset / resolution knobs. Video
//! Compress / Trim / Extract Audio land as separate tools and pick their
//! own recipes; this keeps the v1 surface small while still mirroring the
//! "format dropdown" UX of the audio + image converters.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::ffmpeg;
use crate::fs::unique_path;

/// Target container + codec recipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetFormat {
    /// MP4 container, H.264 video + AAC audio.
    Mp4,
    /// WebM container, VP9 video + Opus audio.
    Webm,
    /// Matroska container, **stream-copy** (no re-encode). Near-instant
    /// remux. Fails if the source streams aren't in mkv's compatible set
    /// (vanishingly rare in practice — mkv is permissive).
    Mkv,
}

impl TargetFormat {
    /// Output file extension (no leading dot).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
            Self::Mkv => "mkv",
        }
    }

    /// Codec / quality args for this target — inserted between
    /// `-i <source>` and `<output>` in the final ffmpeg invocation.
    ///
    /// Rationale per pick:
    /// - **mp4**: H.264 CRF 23 is the x264-documented "visually
    ///   lossless-ish" sweet spot; AAC 128k is the lossy default the rest
    ///   of the audio world uses.
    /// - **webm**: VP9 wants `-b:v 0` to enter constant-quality mode
    ///   (otherwise it caps bitrate); `-row-mt 1` enables row-based
    ///   multithreading for ~2× speedup on multi-core hosts.
    /// - **mkv**: stream copy. Real re-encoding into mkv is the
    ///   Video Compress tool's story, not this one.
    fn codec_args(self) -> &'static [&'static str] {
        match self {
            Self::Mp4 => &[
                "-c:v", "libx264", "-preset", "medium", "-crf", "23", "-c:a", "aac", "-b:a", "128k",
            ],
            Self::Webm => &[
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "32",
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-c:a",
                "libopus",
                "-b:a",
                "96k",
            ],
            Self::Mkv => &["-c:v", "copy", "-c:a", "copy"],
        }
    }
}

/// User-facing options. Only the target format is exposed in v1 — see the
/// brief in `docs/plans/VIDEO_FORMAT_CONVERTER.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Opts {
    pub target_format: TargetFormat,
}

/// Convert a single video file. Builds the ffmpeg arg vec, probes the
/// source's duration so progress can be reported as a 0..=1 fraction, and
/// drives [`crate::ffmpeg::run`] under the supplied cancellation token.
///
/// Returns the collision-resolved output path on success.
///
/// On any error (including cancellation), the partial output file is
/// deleted before returning. A half-written `.mp4` is just garbage; we'd
/// rather leave nothing than something the user might try to play.
///
/// Errors:
/// - `AppError::FileNotFound` if `source` does not exist on disk.
/// - `AppError::Cancelled` if the token fires mid-encode.
/// - `AppError::ProcessingFailed` if ffmpeg exits non-zero (codec error,
///   unreadable container, permission denied on output, …).
pub fn convert<F>(
    source: &Path,
    opts: &Opts,
    mut on_file_progress: F,
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

    let target_path = derive_output_path(source, opts.target_format);
    let final_path = unique_path(&target_path).map_err(|err| AppError::ProcessingFailed {
        detail: format!("derive unique output {}: {err}", target_path.display()),
    })?;

    // Duration is required for the 0..=1 progress fraction. If ffmpeg
    // can't read the input enough to report a Duration line, the run
    // would have failed anyway — surface that as the error.
    let duration_secs = ffmpeg::probe_duration_secs(source)?;

    let args = build_args(source, &final_path, opts.target_format);
    let result = ffmpeg::run(
        &args,
        |p| {
            let fraction = if duration_secs > 0.0 {
                ((p.out_time_us as f64) / 1_000_000.0 / duration_secs).clamp(0.0, 1.0)
            } else {
                0.0
            };
            on_file_progress(fraction);
        },
        cancel,
    );

    match result {
        Ok(()) => Ok(final_path),
        Err(err) => {
            // Best-effort cleanup. If the file wasn't created (ffmpeg
            // failed before opening output), `remove_file` returns
            // NotFound which we ignore.
            let _ = std::fs::remove_file(&final_path);
            Err(err)
        }
    }
}

/// `{stem}_converted.{ext}` next to the source. The `_converted` suffix
/// ensures the output is distinct from the input even when the user
/// "converts" to the same format the source is already in.
pub(super) fn derive_output_path(source: &Path, target: TargetFormat) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = format!("{stem}_converted.{ext}", ext = target.extension());
    source.with_file_name(name)
}

/// Build the full ffmpeg arg vec for one conversion. `-y` overwrites the
/// target if a residue file is somehow already there (it shouldn't be —
/// `unique_path` ran — but `-y` keeps ffmpeg from going interactive on a
/// race).
fn build_args(source: &Path, output: &Path, target: TargetFormat) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::with_capacity(4 + target.codec_args().len());
    args.push("-y".into());
    args.push("-i".into());
    args.push(source.as_os_str().to_os_string());
    args.extend(target.codec_args().iter().map(OsString::from));
    args.push(output.as_os_str().to_os_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extension_per_target() {
        assert_eq!(TargetFormat::Mp4.extension(), "mp4");
        assert_eq!(TargetFormat::Webm.extension(), "webm");
        assert_eq!(TargetFormat::Mkv.extension(), "mkv");
    }

    #[test]
    fn output_path_adds_converted_suffix_next_to_source() {
        let src = PathBuf::from("/videos/holiday.mov");
        let out = derive_output_path(&src, TargetFormat::Mp4);
        assert_eq!(out, PathBuf::from("/videos/holiday_converted.mp4"));
    }

    #[test]
    fn output_path_same_format_still_uses_converted_suffix() {
        // Converting mp4 → mp4 still gets the suffix — that's how we
        // guarantee the output is distinct from the input even before
        // unique_path resolution.
        let src = PathBuf::from("/videos/clip.mp4");
        let out = derive_output_path(&src, TargetFormat::Mp4);
        assert_eq!(out, PathBuf::from("/videos/clip_converted.mp4"));
    }

    #[test]
    fn output_path_handles_multi_dot_stem() {
        // file_stem keeps everything before the LAST dot, so we get
        // "report.final" + "_converted.mp4".
        let src = PathBuf::from("/videos/report.final.mov");
        let out = derive_output_path(&src, TargetFormat::Mp4);
        assert_eq!(out, PathBuf::from("/videos/report.final_converted.mp4"));
    }

    #[test]
    fn mp4_args_carry_libx264_and_aac_with_crf_and_preset() {
        let args = build_args(
            Path::new("/in.mov"),
            Path::new("/out.mp4"),
            TargetFormat::Mp4,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(joined[0], "-y");
        assert_eq!(joined[1], "-i");
        assert_eq!(joined[2], "/in.mov");
        assert!(joined.contains(&"libx264"));
        assert!(joined.contains(&"medium"));
        assert!(joined.contains(&"23"));
        assert!(joined.contains(&"aac"));
        assert!(joined.contains(&"128k"));
        assert_eq!(joined.last().copied(), Some("/out.mp4"));
    }

    #[test]
    fn webm_args_carry_vp9_with_constant_quality_and_opus() {
        let args = build_args(
            Path::new("/in.mov"),
            Path::new("/out.webm"),
            TargetFormat::Webm,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert!(joined.contains(&"libvpx-vp9"));
        assert!(joined.contains(&"32"));
        // `-b:v 0` is the constant-quality mode flip — VP9 caps bitrate
        // by default and this disables that cap.
        let bv_idx = joined.iter().position(|&s| s == "-b:v").unwrap();
        assert_eq!(joined[bv_idx + 1], "0");
        assert!(joined.contains(&"-row-mt"));
        assert!(joined.contains(&"libopus"));
        assert!(joined.contains(&"96k"));
    }

    #[test]
    fn mkv_args_stream_copy_only() {
        let args = build_args(
            Path::new("/in.mov"),
            Path::new("/out.mkv"),
            TargetFormat::Mkv,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let cv_idx = joined.iter().position(|&s| s == "-c:v").unwrap();
        assert_eq!(joined[cv_idx + 1], "copy");
        let ca_idx = joined.iter().position(|&s| s == "-c:a").unwrap();
        assert_eq!(joined[ca_idx + 1], "copy");
        // No CRF / bitrate args — stream copy doesn't take them.
        assert!(!joined.contains(&"-crf"));
        assert!(!joined.contains(&"-b:a"));
    }

    #[test]
    fn build_args_overwrite_flag_is_first() {
        // `-y` must precede `-i` so ffmpeg never goes interactive on a
        // pre-existing output path.
        let args = build_args(
            Path::new("/in.mov"),
            Path::new("/out.mp4"),
            TargetFormat::Mp4,
        );
        assert_eq!(args[0], OsString::from("-y"));
        assert_eq!(args[1], OsString::from("-i"));
    }
}
