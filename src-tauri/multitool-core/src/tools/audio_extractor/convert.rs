//! Audio Extractor — per-track ffmpeg primitive.
//!
//! Owns the recipe (`-vn -map 0:a:<i> -c:a libmp3lame -q:a 2`), the
//! `<stem>_audio[_N].mp3` naming rule, the duration-probe → fraction math,
//! and the partial-file cleanup on failure. The orchestrator in
//! [`super::job`] drives this once per audio track.
//!
//! Recipe rationale:
//! - **`-vn`** drops the video stream entirely so we don't waste decode work
//!   or accidentally produce a video-with-audio output.
//! - **`-map 0:a:<i>`** picks the i-th audio stream by index. `0` because
//!   the single input is `-i <source>`; the `a:<i>` selector is ffmpeg's
//!   stream-type-aware indexing (skips video / subtitle / data streams).
//! - **`libmp3lame -q:a 2`** is LAME's V2 preset, ~190 kbps VBR, broadly
//!   considered transparent for music. Bundled inside the eugeneware
//!   ffmpeg build (see [`crate::ffmpeg`] + the Video stack DECISIONS entry).
//! - **Even-dimension scale filter is intentionally absent** — that's a
//!   libx264 / libvpx-vp9 constraint, not an audio-encode one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::ffmpeg;
use crate::fs::unique_path;

/// Extract one audio track from `source` as MP3.
///
/// - `track_index` is 0-based and feeds the ffmpeg `-map 0:a:<i>` selector.
/// - `track_total` shapes the output filename: single-track sources land at
///   `<stem>_audio.mp3`, multi-track at `<stem>_audio_<1-based>.mp3`.
///
/// Returns the collision-resolved output path on success.
///
/// On any error (including cancellation), the in-flight partial output file
/// is removed before returning — a half-written `.mp3` is just garbage and
/// would mislead the user. Already-extracted tracks from prior iterations
/// of the job orchestrator are not touched.
pub(super) fn extract_one_track<F>(
    source: &Path,
    track_index: u32,
    track_total: u32,
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

    let target_path = derive_output_path(source, track_index, track_total);
    let final_path = unique_path(&target_path).map_err(|err| AppError::ProcessingFailed {
        detail: format!("derive unique output {}: {err}", target_path.display()),
    })?;

    // Duration is needed for the 0..=1 progress fraction. If ffmpeg can't
    // read the source enough to report a Duration line, the encode would
    // have failed anyway — surface that as the error.
    let duration_secs = ffmpeg::probe_duration_secs(source)?;

    let args = build_args(source, &final_path, track_index);
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
            // Best-effort cleanup; NotFound (ffmpeg failed before opening
            // output) is fine to ignore.
            let _ = std::fs::remove_file(&final_path);
            Err(err)
        }
    }
}

/// `<stem>_audio.mp3` (single-track) or `<stem>_audio_<N>.mp3` (multi-track,
/// 1-indexed in the filename for users; the i-based ffmpeg selector stays
/// 0-indexed internally).
pub(super) fn derive_output_path(source: &Path, track_index: u32, track_total: u32) -> PathBuf {
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = if track_total <= 1 {
        format!("{stem}_audio.mp3")
    } else {
        let display_index = track_index.saturating_add(1);
        format!("{stem}_audio_{display_index}.mp3")
    };
    source.with_file_name(name)
}

/// Build the ffmpeg arg vec for one track extraction. `-y` precedes `-i` so
/// ffmpeg never goes interactive on a pre-existing output (shouldn't happen
/// because `unique_path` ran, but it's a cheap belt-and-braces).
fn build_args(source: &Path, output: &Path, track_index: u32) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::with_capacity(10);
    args.push("-y".into());
    args.push("-i".into());
    args.push(source.as_os_str().to_os_string());
    args.push("-vn".into());
    args.push("-map".into());
    args.push(format!("0:a:{track_index}").into());
    args.push("-c:a".into());
    args.push("libmp3lame".into());
    args.push("-q:a".into());
    args.push("2".into());
    args.push(output.as_os_str().to_os_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_track_naming_omits_number() {
        let src = PathBuf::from("/videos/holiday.mov");
        let out = derive_output_path(&src, 0, 1);
        assert_eq!(out, PathBuf::from("/videos/holiday_audio.mp3"));
    }

    #[test]
    fn multi_track_naming_is_one_indexed() {
        let src = PathBuf::from("/videos/concert.mkv");
        // 3 tracks: indices 0, 1, 2 become _audio_1, _audio_2, _audio_3.
        assert_eq!(
            derive_output_path(&src, 0, 3),
            PathBuf::from("/videos/concert_audio_1.mp3"),
        );
        assert_eq!(
            derive_output_path(&src, 1, 3),
            PathBuf::from("/videos/concert_audio_2.mp3"),
        );
        assert_eq!(
            derive_output_path(&src, 2, 3),
            PathBuf::from("/videos/concert_audio_3.mp3"),
        );
    }

    #[test]
    fn naming_handles_multi_dot_stem() {
        // file_stem keeps everything before the LAST dot. `report.final.mov`
        // → stem `report.final`, output `report.final_audio.mp3`.
        let src = PathBuf::from("/videos/report.final.mov");
        let out = derive_output_path(&src, 0, 1);
        assert_eq!(out, PathBuf::from("/videos/report.final_audio.mp3"));
    }

    #[test]
    fn naming_falls_back_to_output_when_stem_unknown() {
        // Path with no stem (root) — defensive fallback so we never emit a
        // bare `_audio.mp3` with no prefix.
        let src = PathBuf::from("/");
        let out = derive_output_path(&src, 0, 1);
        assert_eq!(out, PathBuf::from("/output_audio.mp3"));
    }

    #[test]
    fn args_include_vn_map_and_libmp3lame_recipe() {
        let args = build_args(Path::new("/in.mp4"), Path::new("/out_audio.mp3"), 0);
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(joined[0], "-y");
        assert_eq!(joined[1], "-i");
        assert_eq!(joined[2], "/in.mp4");
        assert!(joined.contains(&"-vn"), "must drop video stream");
        // `-map 0:a:0` picks the first audio stream by stream-type index.
        let map_idx = joined.iter().position(|&s| s == "-map").unwrap();
        assert_eq!(joined[map_idx + 1], "0:a:0");
        assert!(joined.contains(&"libmp3lame"));
        let q_idx = joined.iter().position(|&s| s == "-q:a").unwrap();
        assert_eq!(joined[q_idx + 1], "2", "V2 preset, ~190 kbps VBR");
        assert_eq!(joined.last().copied(), Some("/out_audio.mp3"));
    }

    #[test]
    fn args_use_correct_track_index_for_multi_track_extraction() {
        // 3rd audio track → `-map 0:a:2`.
        let args = build_args(Path::new("/in.mkv"), Path::new("/out_audio_3.mp3"), 2);
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        let map_idx = joined.iter().position(|&s| s == "-map").unwrap();
        assert_eq!(joined[map_idx + 1], "0:a:2");
    }

    #[test]
    fn args_overwrite_flag_is_first() {
        // `-y` must precede `-i` so ffmpeg never goes interactive on a
        // pre-existing output path.
        let args = build_args(Path::new("/in.mp4"), Path::new("/out.mp3"), 0);
        assert_eq!(args[0], OsString::from("-y"));
        assert_eq!(args[1], OsString::from("-i"));
    }
}
