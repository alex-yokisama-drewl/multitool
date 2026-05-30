//! Video Trimmer — single-file pure pipeline.
//!
//! `(source, opts)` in, output `PathBuf` out. Trimming is a
//! **frame-accurate, codec-matched full re-encode**: ffmpeg decodes the
//! source, cuts at the exact requested frame (output seek — `-ss` AFTER
//! `-i`), and re-encodes with a same-family encoder and the source's
//! pixel format so the output round-trips without a silent quality
//! downgrade. Codec/pixfmt come from
//! [`crate::ffmpeg::probe_video_stream_params`] +
//! [`crate::ffmpeg::probe_audio_stream_params`]; the mirror tables live
//! in [`select_video_encoder_flags`] / [`select_audio_encoder_flags`].
//!
//! Why frame-accurate, always-on: the v1 stream-copy snapped the cut to
//! the keyframe ≤ start (up to ~8–10s early on long-GOP sources). Speed
//! doesn't matter for a learning-project trimmer, accuracy does — see
//! [`docs/plans/VIDEO_TRIMMER_FRAME_ACCURATE.md`](../../../../../docs/plans/VIDEO_TRIMMER_FRAME_ACCURATE.md).
//!
//! ffmpeg-specific spawning lives in [`crate::ffmpeg`]; this module owns
//! the trimmer's wire types, the arg-vec builder, the codec-mirror
//! tables, and the output-naming rule.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::ffmpeg::{self, AudioStreamParams, VideoStreamParams};
use crate::fs::unique_path;

/// User-facing trim options. Range bounds are milliseconds from the start
/// of the source.
///
/// `end_ms` is clamped to the probed source duration in [`convert`];
/// `start_ms >= end_ms` (after clamping) is rejected with
/// `ProcessingFailed`. No fades, no codec knobs — codec, pixfmt, and
/// audio encoder are mirrored from the source via the
/// [`select_video_encoder_flags`] / [`select_audio_encoder_flags`]
/// tables.
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

    // Duration both clamps `end_ms` and feeds the progress fraction. If
    // ffmpeg can't read the input enough to report a Duration line, the
    // re-encode would have failed anyway — surface that as the error.
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

    // Probe codec params for the re-encode mirror. A second `ffmpeg -i`
    // banner spawn beyond `probe_duration_secs` above — acceptable;
    // banner probes are sub-100ms each, and the alternative is plumbing
    // a single banner-probe helper, which would muddy the shim's
    // single-purpose APIs.
    let video_params = ffmpeg::probe_video_stream_params(source)?;
    let audio_params = ffmpeg::probe_audio_stream_params(source)?;

    let target_path = derive_output_path(source);
    let final_path = unique_path(&target_path).map_err(|err| AppError::ProcessingFailed {
        detail: format!("derive unique output {}: {err}", target_path.display()),
    })?;

    let args = build_reencode_args(
        source,
        &final_path,
        opts.start_ms,
        trim_ms,
        &video_params,
        audio_params.as_ref(),
    );
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

/// Format a millisecond count as `S.mmm` seconds for ffmpeg's `-ss` / `-t`
/// (e.g. `90250` → `"90.250"`). ffmpeg parses fractional-seconds durations
/// directly, so we never need the `HH:MM:SS` form.
fn fmt_secs(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

// ---------- Codec-mirror tables for the frame-accurate re-encode ----------
//
// The mapping tables below mirror the source codec onto a same-family
// encoder available in the bundled ffmpeg (eugeneware b6.1.1; coverage
// verified per `docs/plans/VIDEO_TRIMMER_FRAME_ACCURATE.md` →
// "Step 1 prerequisite"). CRF values target perceptual transparency for
// a single encoding generation — H.264 18, HEVC 20, VP9 28, AV1 30. We
// do NOT mirror source bitrate (CBR-to-match-source can both bloat *and*
// degrade; relying on CRF lets the codec spend bits where they matter).
//
// `codec` is the lowercase ffmpeg name parsed by
// [`crate::ffmpeg::parse_video_stream_line`] /
// [`crate::ffmpeg::parse_audio_stream_line`].

/// Encoder + quality flags for a source video codec name. Returns the
/// ffmpeg arg fragments in `[-c:v <enc>, <flag>, <val>, …]` order.
/// Anything we don't recognise falls back to libx264 — it's universally
/// playable, ships in every release of the bundle, and lands smaller
/// than the source for most legacy codecs.
fn select_video_encoder_flags(codec: &str) -> &'static [&'static str] {
    match codec {
        "h264" => &["-c:v", "libx264", "-crf", "18", "-preset", "medium"],
        "hevc" => &["-c:v", "libx265", "-crf", "20", "-preset", "medium"],
        "vp9" => &[
            "-c:v",
            "libvpx-vp9",
            "-crf",
            "28",
            "-b:v",
            "0",
            "-row-mt",
            "1",
        ],
        "av1" => &[
            "-c:v",
            "libaom-av1",
            "-crf",
            "30",
            "-cpu-used",
            "6",
            "-row-mt",
            "1",
        ],
        "vp8" => &["-c:v", "libvpx", "-crf", "10", "-b:v", "1M"],
        _ => &["-c:v", "libx264", "-crf", "18", "-preset", "medium"],
    }
}

/// Encoder + quality flags for a source audio codec name. AAC is the
/// universal fallback — it's the lingua franca of mp4/mov containers
/// and rounds out anything mp3-or-older the table doesn't list.
fn select_audio_encoder_flags(codec: &str) -> &'static [&'static str] {
    match codec {
        "aac" => &["-c:a", "aac", "-b:a", "192k"],
        "opus" => &["-c:a", "libopus", "-b:a", "128k"],
        "mp3" => &["-c:a", "libmp3lame", "-q:a", "2"],
        "flac" => &["-c:a", "flac"],
        "vorbis" => &["-c:a", "libvorbis", "-q:a", "5"],
        "ac3" => &["-c:a", "ac3", "-b:a", "192k"],
        _ => &["-c:a", "aac", "-b:a", "192k"],
    }
}

/// Build the ffmpeg arg vec for a **frame-accurate, codec-matched
/// re-encode** trim. Shape change vs [`build_args`]:
///
/// - `-ss` lands **after** `-i` — output seek, frame-accurate (decode
///   from previous keyframe, discard until the requested ms). The v1
///   `-ss` before `-i` was an input seek (fast, keyframe-snapped).
/// - `-map 0:v:0` (single video stream, no `?`). A re-encode can't
///   accept "all video streams" the way `-c copy` could.
/// - `-map 0:a?` only when the source actually has audio — `audio:
///   None` (silent video) drops the audio map and audio codec flags
///   entirely.
/// - Video codec + flags from the mirror table; `-pix_fmt` mirrored
///   from the probed source pix_fmt (so 10-bit / HDR-adjacent sources
///   round-trip without a silent yuv420p downgrade). Falls back to
///   yuv420p only when the probe didn't yield one.
/// - Audio codec + flags from the mirror table when audio is present.
/// - `-avoid_negative_ts make_zero` kept — defends against quirky
///   source timestamps on either trim path.
fn build_reencode_args(
    source: &Path,
    output: &Path,
    start_ms: u64,
    trim_ms: u64,
    video: &VideoStreamParams,
    audio: Option<&AudioStreamParams>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-y".into(),
        "-i".into(),
        source.as_os_str().to_os_string(),
        "-ss".into(),
        fmt_secs(start_ms).into(),
        "-t".into(),
        fmt_secs(trim_ms).into(),
        "-map".into(),
        "0:v:0".into(),
    ];
    if audio.is_some() {
        args.push("-map".into());
        args.push("0:a?".into());
    }
    for &flag in select_video_encoder_flags(&video.codec) {
        args.push(flag.into());
    }
    args.push("-pix_fmt".into());
    args.push(video.pix_fmt.as_deref().unwrap_or("yuv420p").into());
    if let Some(audio) = audio {
        for &flag in select_audio_encoder_flags(&audio.codec) {
            args.push(flag.into());
        }
    }
    args.push("-avoid_negative_ts".into());
    args.push("make_zero".into());
    args.push(output.as_os_str().to_os_string());
    args
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

    fn video_params(codec: &str, pix_fmt: Option<&str>) -> VideoStreamParams {
        VideoStreamParams {
            codec: codec.to_string(),
            pix_fmt: pix_fmt.map(str::to_string),
            width: 1280,
            height: 720,
        }
    }

    fn audio_params(codec: &str) -> AudioStreamParams {
        AudioStreamParams {
            codec: codec.to_string(),
            sample_rate: Some(48_000),
            channels: Some("stereo".into()),
        }
    }

    #[test]
    fn video_encoder_flags_map_known_codecs_to_same_family_encoders() {
        let h264 = select_video_encoder_flags("h264");
        assert_eq!(h264[0], "-c:v");
        assert_eq!(h264[1], "libx264");
        assert!(h264.contains(&"-crf"));

        let hevc = select_video_encoder_flags("hevc");
        assert_eq!(hevc[1], "libx265");

        let vp9 = select_video_encoder_flags("vp9");
        assert_eq!(vp9[1], "libvpx-vp9");
        assert!(vp9.contains(&"-row-mt"), "vp9 needs row-mt for parallelism");

        let av1 = select_video_encoder_flags("av1");
        assert_eq!(av1[1], "libaom-av1");

        let vp8 = select_video_encoder_flags("vp8");
        assert_eq!(vp8[1], "libvpx");
    }

    #[test]
    fn video_encoder_flags_unknown_codec_falls_back_to_libx264() {
        // mpeg2video, wmv3, mpeg4, and anything else exotic — all land
        // on libx264 because it's universally playable and ships in the
        // bundled binary on every target.
        for exotic in ["mpeg2video", "wmv3", "mpeg4", "prores", "dvvideo", ""] {
            let flags = select_video_encoder_flags(exotic);
            assert_eq!(flags[1], "libx264", "fallback for {exotic}");
        }
    }

    #[test]
    fn audio_encoder_flags_map_known_codecs() {
        assert_eq!(select_audio_encoder_flags("aac")[1], "aac");
        assert_eq!(select_audio_encoder_flags("opus")[1], "libopus");
        assert_eq!(select_audio_encoder_flags("mp3")[1], "libmp3lame");
        assert_eq!(select_audio_encoder_flags("flac")[1], "flac");
        assert_eq!(select_audio_encoder_flags("vorbis")[1], "libvorbis");
        assert_eq!(select_audio_encoder_flags("ac3")[1], "ac3");
    }

    #[test]
    fn audio_encoder_flags_unknown_codec_falls_back_to_aac() {
        for exotic in ["pcm_s16le", "wmav2", "dts", ""] {
            let flags = select_audio_encoder_flags(exotic);
            assert_eq!(flags[1], "aac", "fallback for {exotic}");
        }
    }

    #[test]
    fn reencode_args_place_ss_after_i_for_frame_accurate_seek() {
        // The whole point of the re-engine: -ss AFTER -i is output seek
        // (frame-accurate), not before -i (input seek, keyframe-snapped).
        let video = video_params("h264", Some("yuv420p"));
        let args = build_reencode_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            1_500,
            4_000,
            &video,
            Some(&audio_params("aac")),
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let i_idx = joined.iter().position(|&s| s == "-i").unwrap();
        let ss_idx = joined.iter().position(|&s| s == "-ss").unwrap();
        assert!(
            ss_idx > i_idx,
            "-ss must come AFTER -i for frame-accurate output seek"
        );
        assert_eq!(joined[ss_idx + 1], "1.500");
        let t_idx = joined.iter().position(|&s| s == "-t").unwrap();
        assert_eq!(joined[t_idx + 1], "4.000");
    }

    #[test]
    fn reencode_args_carry_codec_specific_video_flags_from_mirror_table() {
        let video = video_params("hevc", Some("yuv420p10le"));
        let args = build_reencode_args(
            Path::new("/in.mkv"),
            Path::new("/out.mkv"),
            0,
            1_000,
            &video,
            None,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let cv_idx = joined.iter().position(|&s| s == "-c:v").unwrap();
        assert_eq!(joined[cv_idx + 1], "libx265");
        assert!(joined.contains(&"-crf"));
    }

    #[test]
    fn reencode_args_mirror_pix_fmt_from_source() {
        let video = video_params("hevc", Some("yuv420p10le"));
        let args = build_reencode_args(
            Path::new("/in.mkv"),
            Path::new("/out.mkv"),
            0,
            1_000,
            &video,
            None,
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
    fn reencode_args_pix_fmt_falls_back_to_yuv420p_when_probe_returned_none() {
        let video = video_params("h264", None);
        let args = build_reencode_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            0,
            1_000,
            &video,
            None,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let pix_idx = joined.iter().position(|&s| s == "-pix_fmt").unwrap();
        assert_eq!(joined[pix_idx + 1], "yuv420p");
    }

    #[test]
    fn reencode_args_omit_audio_map_and_codec_for_silent_source() {
        let video = video_params("h264", Some("yuv420p"));
        let args = build_reencode_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            0,
            1_000,
            &video,
            None,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert!(
            !joined.windows(2).any(|w| w == ["-map", "0:a?"]),
            "silent video must not map audio"
        );
        assert!(
            !joined.contains(&"-c:a"),
            "silent video must not set an audio codec"
        );
    }

    #[test]
    fn reencode_args_carry_audio_codec_flags_for_audio_source() {
        let video = video_params("h264", Some("yuv420p"));
        let args = build_reencode_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            0,
            1_000,
            &video,
            Some(&audio_params("opus")),
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let ca_idx = joined.iter().position(|&s| s == "-c:a").unwrap();
        assert_eq!(joined[ca_idx + 1], "libopus");
        assert!(joined.windows(2).any(|w| w == ["-map", "0:a?"]));
    }

    #[test]
    fn reencode_args_map_single_video_stream_only_no_optional_marker() {
        // `-c copy` can take `0:v?` (all video streams); a re-encode
        // can't — narrow to the first video stream explicitly.
        let video = video_params("h264", Some("yuv420p"));
        let args = build_reencode_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            0,
            1_000,
            &video,
            None,
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert!(joined.windows(2).any(|w| w == ["-map", "0:v:0"]));
        assert!(!joined.windows(2).any(|w| w == ["-map", "0:v?"]));
    }

    #[test]
    fn reencode_args_rebase_timestamps_with_avoid_negative_ts_make_zero() {
        let video = video_params("h264", Some("yuv420p"));
        let args = build_reencode_args(
            Path::new("/in.mp4"),
            Path::new("/out.mp4"),
            0,
            1_000,
            &video,
            Some(&audio_params("aac")),
        );
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let ats_idx = joined
            .iter()
            .position(|&s| s == "-avoid_negative_ts")
            .unwrap();
        assert_eq!(joined[ats_idx + 1], "make_zero");
    }
}
