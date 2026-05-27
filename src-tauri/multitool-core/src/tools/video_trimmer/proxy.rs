//! Preview-proxy transcode for the Video Trimmer.
//!
//! The trimmer's player must show frames for *any* source, but the WebView
//! can't always decode the source natively. When native playback fails,
//! the UI asks for a **proxy**: a throwaway, web-friendly clip transcoded
//! from the source and played in place of it. The trim itself always runs
//! against the *original* file (see [`super::convert`]) — the proxy is
//! preview-only and is deleted once the user moves on.
//!
//! **Why WebM/VP9/Opus, not mp4/H.264:** the Linux WebView (WebKitGTK)
//! decodes H.264/AAC only when proprietary gstreamer plugins
//! (`gst-libav` / `gst-plugins-bad`) are installed, which they often
//! aren't — so an H.264 proxy fails to play exactly where we need the
//! fallback most. VP9 + Opus in WebM are the open codecs WebKitGTK ships
//! with out of the box (and every other target WebView decodes them too).
//!
//! The recipe favors speed over fidelity (it's discarded): VP9 in
//! `-deadline realtime -cpu-used 8` mode, constant-quality `-crf 32`,
//! downscaled to ≤1280px wide, Opus audio.

use std::ffi::OsString;
use std::path::Path;

use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::ffmpeg;

/// Transcode `source` into a web-friendly mp4 at `dest`, reporting a
/// 0..=1 progress fraction. The caller (the Tauri shell) owns `dest` —
/// typically a per-pick path in the OS temp dir.
///
/// On any error (including cancellation) the partial proxy at `dest` is
/// deleted before returning.
///
/// Errors:
/// - `AppError::FileNotFound` if `source` does not exist on disk.
/// - `AppError::ProcessingFailed` if ffmpeg can't probe a duration or the
///   transcode exits non-zero.
/// - `AppError::Cancelled` if the token fires mid-transcode.
pub fn generate_proxy<F>(
    source: &Path,
    dest: &Path,
    mut on_progress: F,
    cancel: &CancellationToken,
) -> AppResult<()>
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

    let duration_secs = ffmpeg::probe_duration_secs(source)?;

    let args = build_proxy_args(source, dest);
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
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(dest);
            Err(err)
        }
    }
}

/// Build the ffmpeg arg vec for a preview proxy. VP9 in `-deadline
/// realtime -cpu-used 8` mode trades fidelity for encode speed (throwaway
/// output); `-b:v 0 -crf 32` is VP9's constant-quality mode and `-row-mt 1`
/// enables row multithreading. The scale filter caps the width at 1280
/// without upscaling (`min(1280,iw)`, comma escaped so the filtergraph
/// parser doesn't read it as a filter separator) and forces an even height
/// (`-2`). `-pix_fmt yuv420p` is the chroma layout every WebView decodes.
/// Opus audio. `-y` overwrites any stale proxy at `dest`.
fn build_proxy_args(source: &Path, dest: &Path) -> Vec<OsString> {
    vec![
        "-y".into(),
        "-i".into(),
        source.as_os_str().to_os_string(),
        "-vf".into(),
        r"scale=min(1280\,iw):-2".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:v".into(),
        "libvpx-vp9".into(),
        "-deadline".into(),
        "realtime".into(),
        "-cpu-used".into(),
        "8".into(),
        "-b:v".into(),
        "0".into(),
        "-crf".into(),
        "32".into(),
        "-row-mt".into(),
        "1".into(),
        "-c:a".into(),
        "libopus".into(),
        "-b:a".into(),
        "96k".into(),
        dest.as_os_str().to_os_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn proxy_args_carry_open_codec_recipe() {
        let args = build_proxy_args(Path::new("/in.mkv"), Path::new("/out.webm"));
        let joined: Vec<&str> = args.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(joined[0], "-y");
        assert_eq!(joined[1], "-i");
        assert_eq!(joined[2], "/in.mkv");
        // Open codecs WebKitGTK decodes without proprietary plugins.
        assert!(joined.contains(&"libvpx-vp9"));
        assert!(joined.contains(&"libopus"));
        assert!(!joined.contains(&"libx264"));
        assert!(!joined.contains(&"aac"));
        // Realtime, constant-quality VP9.
        assert!(joined.contains(&"realtime"));
        assert!(joined.contains(&"32"));
        let bv_idx = joined.iter().position(|&s| s == "-b:v").unwrap();
        assert_eq!(joined[bv_idx + 1], "0");
        assert!(joined.contains(&"yuv420p"));
        // Width cap with the comma escaped for the filtergraph parser.
        let vf_idx = joined.iter().position(|&s| s == "-vf").unwrap();
        assert_eq!(joined[vf_idx + 1], r"scale=min(1280\,iw):-2");
        assert_eq!(joined.last().copied(), Some("/out.webm"));
    }

    #[test]
    fn missing_source_is_file_not_found() {
        let dir = TempDir::new().unwrap();
        let result = generate_proxy(
            &PathBuf::from("/no/such/clip.mkv"),
            &dir.path().join("proxy.mp4"),
            |_| {},
            &CancellationToken::new(),
        );
        assert!(matches!(result, Err(AppError::FileNotFound { .. })));
    }

    #[test]
    fn generates_a_playable_proxy_from_a_real_clip() {
        // Synthesize a 1s clip, transcode a proxy, confirm it exists and
        // is itself probe-able (i.e. a valid mp4 the recipe actually
        // produced — catches a broken filter/arg string).
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.mp4");
        let src_str = src.to_str().unwrap();
        let synth = [
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=64x64:rate=10",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            src_str,
        ];
        ffmpeg::run(synth, |_| {}, &CancellationToken::new()).expect("synth clip");

        let proxy = dir.path().join("proxy.webm");
        let saw_progress = std::cell::Cell::new(false);
        generate_proxy(
            &src,
            &proxy,
            |f| {
                if f > 0.0 {
                    saw_progress.set(true);
                }
            },
            &CancellationToken::new(),
        )
        .expect("proxy ok");

        assert!(proxy.exists());
        assert!(std::fs::metadata(&proxy).unwrap().len() > 0);
        // Proxy is a real, readable mp4.
        assert!(ffmpeg::probe_duration_secs(&proxy).is_ok());
    }

    #[test]
    fn garbage_source_fails_and_leaves_no_partial_proxy() {
        let dir = TempDir::new().unwrap();
        let bad = dir.path().join("bad.mkv");
        std::fs::write(&bad, b"not a video").unwrap();
        let proxy = dir.path().join("proxy.webm");

        let result = generate_proxy(&bad, &proxy, |_| {}, &CancellationToken::new());
        assert!(matches!(result, Err(AppError::ProcessingFailed { .. })));
        assert!(!proxy.exists(), "partial proxy should be unlinked");
    }
}
