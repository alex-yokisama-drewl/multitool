//! Source-duration probe for the Video Trimmer UI.
//!
//! The picked-file view needs the source duration up front to place the
//! end marker and clamp the time inputs — and `<video>.duration` is
//! unreliable / unavailable when the source isn't WebView-decodable (the
//! proxy-preview case). So the duration always comes from the backend via
//! this thin wrapper over [`crate::ffmpeg::probe_duration_secs`].

use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::ffmpeg;

/// Probe the source's duration in **milliseconds**.
///
/// Errors:
/// - `AppError::FileNotFound` if `source` does not exist on disk.
/// - `AppError::ProcessingFailed` if ffmpeg can't report a `Duration:`
///   line (unreadable / not a media file).
pub fn probe_duration_ms(source: &Path) -> AppResult<u64> {
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

    let secs = ffmpeg::probe_duration_secs(source)?;
    Ok((secs * 1000.0).round().max(0.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn missing_source_is_file_not_found() {
        let result = probe_duration_ms(&PathBuf::from("/no/such/clip.mp4"));
        assert!(matches!(result, Err(AppError::FileNotFound { .. })));
    }

    #[test]
    fn probes_a_real_clips_duration_in_ms() {
        // Synthesize a 2s clip and confirm the probe reports ~2000 ms.
        let dir = TempDir::new().unwrap();
        let clip = dir.path().join("clip.mp4");
        let clip_str = clip.to_str().unwrap();
        let args = [
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=64x64:rate=10",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            clip_str,
        ];
        crate::ffmpeg::run(args, |_| {}, &CancellationToken::new()).expect("synth clip");

        let ms = probe_duration_ms(&clip).expect("probe ok");
        assert!((1_900..=2_200).contains(&ms), "expected ~2000 ms, got {ms}");
    }

    #[test]
    fn unreadable_source_is_processing_failed() {
        let dir = TempDir::new().unwrap();
        let bad = dir.path().join("bad.mp4");
        std::fs::write(&bad, b"not a video").unwrap();
        let result = probe_duration_ms(&bad);
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
    }
}
