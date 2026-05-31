//! Video Compress — single-file pure pipeline.
//!
//! `(source, opts)` in, output `PathBuf` out. Compressing is a
//! **codec-matched full re-encode** at a CRF interpolated from the
//! user-supplied `quality` slider (`0.0` = smallest file / worst
//! quality, `1.0` = highest quality / matches the trimmer's per-codec
//! transparent CRF). The output preserves the source's container +
//! video codec family; audio is stream-copied bit-perfect — there's no
//! audio re-encode in v1, so multi-audio sources keep every track
//! intact. The "scale audio bitrate with the slider" follow-up lives
//! in [BACKLOG.md](../../../../../../docs/plans/BACKLOG.md).
//!
//! Codec/pixfmt are probed from the source via
//! [`crate::ffmpeg::probe_video_stream_params`]; the CRF range +
//! encoder flags come from [`crate::video::reencode`]. Output naming +
//! collision handling via [`crate::fs::unique_path`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::ffmpeg::{self, VideoStreamParams};
use crate::fs::unique_path;
use crate::video::reencode::{video_crf_range, video_encoder_flags};

/// User-facing compress options.
///
/// `quality` is the slider value: `0.0` = smallest file / worst
/// quality, `1.0` = highest quality (matches the trimmer's transparent
/// CRF for the source codec). Out-of-range values clamp silently — the
/// UI slider is bound to `[0, 1]` anyway, so a stray value over the
/// wire is a defensive case, not an error.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Opts {
    pub quality: f64,
}

/// Compress a single video file at the supplied quality. Probes the
/// source's duration (to drive the 0..=1 progress fraction) and codec
/// (to pick the CRF range), builds the ffmpeg arg vec, and drives
/// [`crate::ffmpeg::run`] under the supplied cancellation token.
///
/// Returns the collision-resolved output path on success.
///
/// On any error (including cancellation) the partial output file is
/// deleted before returning — a half-written video is just garbage.
///
/// Errors:
/// - `AppError::FileNotFound` if `source` does not exist on disk.
/// - `AppError::ProcessingFailed` if ffmpeg can't probe a duration or
///   codec, or if the encode exits non-zero.
/// - `AppError::Cancelled` if the token fires mid-encode.
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

    // Duration drives the progress fraction; if ffmpeg can't read it the
    // re-encode would fail anyway — surface the probe error.
    let duration_secs = ffmpeg::probe_duration_secs(source)?;
    let video_params = ffmpeg::probe_video_stream_params(source)?;
    let audio_params = ffmpeg::probe_audio_stream_params(source)?;

    let crf = quality_to_crf(&video_params.codec, opts.quality);

    let target_path = derive_output_path(source);
    let final_path = unique_path(&target_path).map_err(|err| AppError::ProcessingFailed {
        detail: format!("derive unique output {}: {err}", target_path.display()),
    })?;

    let args = build_compress_args(
        source,
        &final_path,
        &video_params,
        audio_params.is_some(),
        crf,
    );

    let result = ffmpeg::run(
        &args,
        |p| {
            let fraction = if duration_secs > 0.0 {
                ((p.out_time_us as f64) / 1_000_000.0 / duration_secs).clamp(0.0, 1.0)
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
            let _ = std::fs::remove_file(&final_path);
            Err(err)
        }
    }
}

/// Map the slider value to a CRF for the supplied source codec.
/// `quality=0.0` → `worst` CRF (smallest file), `quality=1.0` → `best`
/// CRF (highest quality, matches the trimmer's transparent setting).
/// Out-of-range inputs clamp.
///
/// CRF is inverted (smaller CRF = higher quality), so the lerp walks
/// `worst → best` as `quality` goes `0 → 1`.
pub(super) fn quality_to_crf(codec: &str, quality: f64) -> u32 {
    let q = quality.clamp(0.0, 1.0);
    let (best, worst) = video_crf_range(codec);
    let crf = worst as f64 - (worst as f64 - best as f64) * q;
    crf.round() as u32
}

/// `{stem}_compressed.{ext}` next to the source, **before** `unique_path`
/// resolution. Mirrors the trimmer's `{stem}_trimmed.{ext}` shape. A
/// missing extension (picker filter should make this unreachable) falls
/// back to a bare `{stem}_compressed`.
pub(super) fn derive_output_path(source: &Path) -> PathBuf {
    let parent = source.parent();
    let mut name = source.file_stem().map(|s| s.to_owned()).unwrap_or_default();
    name.push("_compressed");
    if let Some(ext) = source.extension() {
        name.push(".");
        name.push(ext);
    }
    match parent {
        Some(p) => p.join(name),
        None => PathBuf::from(name),
    }
}

/// Build the ffmpeg arg vec for a codec-matched compress.
///
/// Shape vs the trimmer's `build_reencode_args`:
/// - No `-ss` / `-t` — we compress the whole source.
/// - `-map 0:v:0` (single video stream — re-encode can't take `0:v?`).
/// - `-map 0:a` only when audio is present. We map *all* audio streams
///   (no `:0`) and stream-copy them — multi-audio sources keep every
///   track bit-perfect. Audio re-encoding is a backlog follow-up.
/// - Video codec + flags from [`video_encoder_flags`] at the
///   slider-interpolated `crf`; `-pix_fmt` mirrored from the probed
///   source pix_fmt (no silent 10-bit → 8-bit downgrade). Falls back
///   to `yuv420p` only when the probe didn't yield one.
/// - `-c:a copy` follows the audio map when audio is present.
/// - No `-avoid_negative_ts` (we don't seek into the middle of the
///   source — timestamps stay as-is).
fn build_compress_args(
    source: &Path,
    output: &Path,
    video: &VideoStreamParams,
    has_audio: bool,
    crf: u32,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-y".into(),
        "-i".into(),
        source.as_os_str().to_os_string(),
        "-map".into(),
        "0:v:0".into(),
    ];
    if has_audio {
        args.push("-map".into());
        args.push("0:a".into());
    }
    for flag in video_encoder_flags(&video.codec, crf) {
        args.push(flag.into());
    }
    args.push("-pix_fmt".into());
    args.push(video.pix_fmt.as_deref().unwrap_or("yuv420p").into());
    if has_audio {
        args.push("-c:a".into());
        args.push("copy".into());
    }
    args.push(output.as_os_str().to_os_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_params(codec: &str, pix_fmt: Option<&str>) -> VideoStreamParams {
        VideoStreamParams {
            codec: codec.to_string(),
            pix_fmt: pix_fmt.map(str::to_string),
            width: 1280,
            height: 720,
        }
    }

    #[test]
    fn output_path_appends_compressed_before_extension() {
        let src = PathBuf::from("/videos/holiday.mp4");
        let out = derive_output_path(&src);
        assert_eq!(out, PathBuf::from("/videos/holiday_compressed.mp4"));
    }

    #[test]
    fn output_path_handles_multi_dot_stem() {
        let src = PathBuf::from("/videos/report.final.mov");
        let out = derive_output_path(&src);
        assert_eq!(out, PathBuf::from("/videos/report.final_compressed.mov"));
    }

    #[test]
    fn output_path_without_extension_falls_back_to_bare_stem() {
        let src = PathBuf::from("/videos/clip");
        let out = derive_output_path(&src);
        assert_eq!(out, PathBuf::from("/videos/clip_compressed"));
    }

    #[test]
    fn quality_one_maps_to_codecs_transparent_crf() {
        // quality=1.0 must hit the `best` slot of the CRF range — the
        // contract that makes "Higher quality" on the slider match the
        // trimmer's perceptually-transparent output.
        for codec in ["h264", "hevc", "vp9", "av1", "vp8"] {
            let (best, _) = video_crf_range(codec);
            assert_eq!(
                quality_to_crf(codec, 1.0),
                best,
                "{codec} should map quality=1.0 to its transparent CRF"
            );
        }
    }

    #[test]
    fn quality_zero_maps_to_codecs_worst_crf() {
        for codec in ["h264", "hevc", "vp9", "av1", "vp8"] {
            let (_, worst) = video_crf_range(codec);
            assert_eq!(
                quality_to_crf(codec, 0.0),
                worst,
                "{codec} should map quality=0.0 to its smallest-file CRF"
            );
        }
    }

    #[test]
    fn quality_midpoint_lands_between_best_and_worst() {
        // The lerp is monotonic, so the midpoint must sit strictly
        // between the two anchors (and not collapse onto either).
        for codec in ["h264", "hevc", "vp9", "av1"] {
            let (best, worst) = video_crf_range(codec);
            let mid = quality_to_crf(codec, 0.5);
            assert!(mid > best && mid < worst, "{codec} midpoint out of bounds");
        }
    }

    #[test]
    fn quality_clamps_out_of_range_values() {
        // Slider is bounded in the UI; defending against a stray value
        // over the IPC keeps the encode reasonable rather than failing.
        let (best, worst) = video_crf_range("h264");
        assert_eq!(quality_to_crf("h264", -1.0), worst);
        assert_eq!(quality_to_crf("h264", 5.0), best);
    }

    #[test]
    fn quality_falls_back_to_h264_range_for_unknown_codec() {
        let (best_h264, worst_h264) = video_crf_range("h264");
        assert_eq!(quality_to_crf("mpeg2video", 1.0), best_h264);
        assert_eq!(quality_to_crf("mpeg2video", 0.0), worst_h264);
    }

    #[test]
    fn compress_args_have_no_ss_or_t() {
        // Compress encodes the whole source — any -ss / -t in the vec
        // would silently chop the output.
        let video = video_params("h264", Some("yuv420p"));
        let args = build_compress_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            &video,
            true,
            23,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert!(!joined.contains(&"-ss"), "compress must not seek-from");
        assert!(!joined.contains(&"-t"), "compress must not duration-cap");
    }

    #[test]
    fn compress_args_carry_codec_specific_video_flags_at_supplied_crf() {
        let video = video_params("hevc", Some("yuv420p10le"));
        let args = build_compress_args(
            Path::new("/in.mkv"),
            Path::new("/out.mkv"),
            &video,
            false,
            27,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let cv_idx = joined.iter().position(|&s| s == "-c:v").unwrap();
        assert_eq!(joined[cv_idx + 1], "libx265");
        let crf_idx = joined.iter().position(|&s| s == "-crf").unwrap();
        assert_eq!(joined[crf_idx + 1], "27");
    }

    #[test]
    fn compress_args_mirror_pix_fmt_from_source() {
        let video = video_params("hevc", Some("yuv420p10le"));
        let args = build_compress_args(
            Path::new("/in.mkv"),
            Path::new("/out.mkv"),
            &video,
            false,
            27,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let pix_idx = joined.iter().position(|&s| s == "-pix_fmt").unwrap();
        assert_eq!(
            joined[pix_idx + 1],
            "yuv420p10le",
            "10-bit source must NOT silently downgrade to yuv420p"
        );
    }

    #[test]
    fn compress_args_pix_fmt_falls_back_to_yuv420p_when_probe_returned_none() {
        let video = video_params("h264", None);
        let args = build_compress_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            &video,
            false,
            23,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let pix_idx = joined.iter().position(|&s| s == "-pix_fmt").unwrap();
        assert_eq!(joined[pix_idx + 1], "yuv420p");
    }

    #[test]
    fn compress_args_omit_audio_map_and_codec_for_silent_source() {
        let video = video_params("h264", Some("yuv420p"));
        let args = build_compress_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            &video,
            false,
            23,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert!(
            !joined.windows(2).any(|w| w == ["-map", "0:a"]),
            "silent video must not map audio"
        );
        assert!(
            !joined.contains(&"-c:a"),
            "silent video must not set an audio codec"
        );
    }

    #[test]
    fn compress_args_stream_copy_audio_when_present() {
        let video = video_params("h264", Some("yuv420p"));
        let args = build_compress_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            &video,
            true,
            23,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        // Audio streams are mapped (all of them, no `:0` selector — the
        // compressor keeps multi-audio sources bit-perfect).
        assert!(joined.windows(2).any(|w| w == ["-map", "0:a"]));
        let ca_idx = joined.iter().position(|&s| s == "-c:a").unwrap();
        assert_eq!(
            joined[ca_idx + 1],
            "copy",
            "audio must be stream-copied in v1"
        );
    }

    #[test]
    fn compress_args_map_single_video_stream_only() {
        let video = video_params("h264", Some("yuv420p"));
        let args = build_compress_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            &video,
            true,
            23,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert!(joined.windows(2).any(|w| w == ["-map", "0:v:0"]));
        assert!(!joined.windows(2).any(|w| w == ["-map", "0:v?"]));
    }

    #[test]
    fn compress_args_unknown_codec_falls_back_to_libx264() {
        let video = video_params("prores", Some("yuv422p10le"));
        let args = build_compress_args(
            Path::new("/in.mov"),
            Path::new("/out.mov"),
            &video,
            false,
            25,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let cv_idx = joined.iter().position(|&s| s == "-c:v").unwrap();
        assert_eq!(joined[cv_idx + 1], "libx264");
    }
}
