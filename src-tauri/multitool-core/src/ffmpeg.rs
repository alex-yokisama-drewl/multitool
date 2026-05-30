//! Process-wide accessor + driver for the bundled ffmpeg sidecar binary.
//!
//! Three surfaces:
//!
//! 1. **Path resolution.** [`init`] sets the binary path at runtime (the Tauri
//!    shell hands in the bundled resource path); the fallback is the
//!    compile-time `FFMPEG_BIN_PATH` baked by [`build.rs`](../../build.rs).
//!    Mirrors [`crate::pdfium`].
//! 2. **[`run`].** Spawns ffmpeg with the standard
//!    `-progress pipe:1 -nostats -hide_banner -loglevel error` prefix,
//!    streams throttled [`FfmpegProgress`] events to the supplied callback,
//!    keeps the last ~64 stderr lines in a ring buffer for failure detail,
//!    and kills + reaps the child if the supplied [`CancellationToken`] fires.
//! 3. **[`probe_duration_secs`] / [`probe_audio_stream_count`] /
//!    [`probe_video_stream_params`] / [`probe_audio_stream_params`].**
//!    Invoke `ffmpeg -i <path>` and parse the `Duration:` / `Stream #...:
//!    Video:` / `Stream #...: Audio:` lines from stderr. No `ffprobe` is
//!    bundled (saves ~50–80 MB per installed bundle); all four line
//!    formats have been stable in ffmpeg for over a decade.
//!
//! The shim is the only place in the tree that knows ffmpeg's CLI shape.
//! Per-tool code (Video Format Converter today; the rest of the video
//! roadmap tomorrow) builds an arg vec and calls [`run`].

use std::collections::VecDeque;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::error::AppError;

static OVERRIDE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Minimum gap between successive [`FfmpegProgress`] callback invocations.
/// ffmpeg's `-progress pipe:1` chatter is sub-100ms on a fast encode; without
/// throttling the UI would be flooded with IPC events.
const PROGRESS_THROTTLE: Duration = Duration::from_millis(250);

/// Cap on the number of stderr lines kept around to attach to a failure
/// detail. Enough to surface the actual codec/format error ffmpeg prints
/// without arbitrary stderr growth pinning memory.
const STDERR_RING_CAPACITY: usize = 64;

/// One progress sample parsed from `-progress pipe:1` output. We only care
/// about `out_time_us` today — the other keys (`bitrate`, `total_size`,
/// `progress`, …) are ignored. A `progress=end` marker arrives as the final
/// line and closes the pipe; that's how the read loop exits cleanly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FfmpegProgress {
    /// Output media time emitted so far, in microseconds. Divide by the
    /// total duration (see [`probe_duration_secs`]) to get a 0..=1 fraction.
    pub out_time_us: u64,
}

/// Override the bundled ffmpeg binary path before any [`run`] /
/// [`probe_duration_secs`] call. Subsequent calls are no-ops — the path is
/// locked in for the lifetime of the process.
///
/// The Tauri shell calls this from its setup hook with the bundled resource
/// path. If the override isn't set, [`binary_path`] falls back to the
/// compile-time `FFMPEG_BIN_PATH` env var baked by `build.rs`, which points
/// into the developer's `OUT_DIR` copy.
pub fn init(path: PathBuf) {
    let _ = OVERRIDE_PATH.set(path);
}

/// Resolved ffmpeg binary path — runtime override (preferred) or
/// compile-time `OUT_DIR` fallback.
fn binary_path() -> PathBuf {
    OVERRIDE_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(env!("FFMPEG_BIN_PATH")))
}

/// Spawn the bundled ffmpeg with the given args, streaming throttled progress
/// events to `on_progress` and aborting on `cancel`.
///
/// The standard `-progress pipe:1 -nostats -hide_banner -loglevel error`
/// prefix is prepended automatically — callers should not pass these
/// themselves. Callers supply the input(s), filter / codec opts, and
/// output path(s).
///
/// Returns:
/// - `Ok(())` on normal zero-exit.
/// - `Err(AppError::Cancelled)` if the token fired before the child exited.
/// - `Err(AppError::ProcessingFailed { detail })` on non-zero exit; `detail`
///   carries the last [`STDERR_RING_CAPACITY`] stderr lines.
pub fn run<I, S, F>(args: I, mut on_progress: F, cancel: &CancellationToken) -> Result<(), AppError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
    F: FnMut(FfmpegProgress),
{
    let bin = binary_path();
    let mut command = Command::new(&bin);
    command
        .args([
            "-progress",
            "pipe:1",
            "-nostats",
            "-hide_banner",
            "-loglevel",
            "error",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    suppress_console_window(&mut command);

    let mut child = command.spawn().map_err(|err| AppError::ProcessingFailed {
        detail: format!("failed to spawn ffmpeg at {}: {err}", bin.display()),
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::ProcessingFailed {
            detail: "ffmpeg stdout pipe missing after spawn".into(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::ProcessingFailed {
            detail: "ffmpeg stderr pipe missing after spawn".into(),
        })?;

    // Drain stderr on a dedicated thread into a ring buffer. ffmpeg's stderr
    // can exceed the OS pipe buffer (~64KB on Linux) if we never read it,
    // which would deadlock the child. The thread exits on EOF (i.e. when
    // the child closes its stderr — either after normal exit or after we
    // kill it).
    let stderr_handle = thread::spawn(move || {
        let mut buf: VecDeque<String> = VecDeque::with_capacity(STDERR_RING_CAPACITY);
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if buf.len() == STDERR_RING_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(line);
        }
        buf
    });

    let mut last_emit = Instant::now()
        .checked_sub(PROGRESS_THROTTLE)
        .unwrap_or_else(Instant::now);
    let mut latest = FfmpegProgress::default();
    let mut saw_any_progress = false;
    let mut cancelled = false;

    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if cancel.is_cancelled() {
            cancelled = true;
            break;
        }
        if let Some(progress) = parse_progress_line(&line) {
            latest = progress;
            saw_any_progress = true;
            if last_emit.elapsed() >= PROGRESS_THROTTLE {
                on_progress(latest);
                last_emit = Instant::now();
            }
        }
    }

    if cancelled {
        let _ = child.kill();
        let _ = child.wait();
        let _ = stderr_handle.join();
        return Err(AppError::Cancelled);
    }

    // Final emit so the caller always sees the last out_time, even if the
    // throttle gate would otherwise have suppressed it.
    if saw_any_progress {
        on_progress(latest);
    }

    let status = child.wait().map_err(|err| AppError::ProcessingFailed {
        detail: format!("ffmpeg wait failed: {err}"),
    })?;
    let stderr_tail: Vec<String> = stderr_handle
        .join()
        .unwrap_or_default()
        .into_iter()
        .collect();

    if status.success() {
        Ok(())
    } else {
        Err(AppError::ProcessingFailed {
            detail: format!("ffmpeg exited with {status}: {}", stderr_tail.join("\n")),
        })
    }
}

/// Probe the duration of a media file in seconds by parsing the `Duration:`
/// line from `ffmpeg -i <path>`'s stderr. ffmpeg exits non-zero with no
/// output target, so we ignore exit status and rely on stderr alone.
pub fn probe_duration_secs(path: &Path) -> Result<f64, AppError> {
    let bin = binary_path();
    let mut command = Command::new(&bin);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    suppress_console_window(&mut command);

    let output = command.output().map_err(|err| AppError::ProcessingFailed {
        detail: format!("failed to spawn ffmpeg at {}: {err}", bin.display()),
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_duration_line(&stderr).ok_or_else(|| AppError::ProcessingFailed {
        detail: format!(
            "ffmpeg -i {} did not report a Duration line",
            path.display()
        ),
    })
}

/// Source video-stream parameters parsed from the first `Stream #...:
/// Video:` line in `ffmpeg -i <path>`'s stderr. Drives the Video
/// Trimmer's codec-matched re-encode — `codec` selects the encoder
/// (`h264` → `libx264`, `hevc` → `libx265`, …) and `pix_fmt` is mirrored
/// directly to the encoder's `-pix_fmt` flag so 10-bit / HDR-adjacent
/// sources don't silently downgrade to yuv420p.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoStreamParams {
    /// Lowercase ffmpeg codec name as it appears in the banner: `h264`,
    /// `hevc`, `vp9`, `av1`, `vp8`, `mpeg4`, `mpeg2video`, `wmv3`, …
    pub codec: String,
    /// Pixel format string, e.g. `yuv420p` / `yuv420p10le` / `yuv444p`.
    /// `None` when the banner's pixfmt slot is malformed (very rare —
    /// some exotic containers).
    pub pix_fmt: Option<String>,
    pub width: u32,
    pub height: u32,
}

/// Source audio-stream parameters parsed from the FIRST `Stream #...:
/// Audio:` line. Multi-track sources (5.1 + commentary + dub) drop to
/// track 0's codec for the mirror — re-encoding three tracks with three
/// different encoders isn't worth the complexity for a v1 trimmer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioStreamParams {
    /// Lowercase ffmpeg codec name: `aac`, `opus`, `mp3`, `flac`,
    /// `vorbis`, `ac3`, …
    pub codec: String,
    /// Sample rate in Hz, when the banner reports one. `None` when the
    /// rate slot is `N/A` or missing.
    pub sample_rate: Option<u32>,
    /// Channel layout as ffmpeg prints it: `mono`, `stereo`, `5.1`,
    /// `7.1`, …
    pub channels: Option<String>,
}

/// Probe the first video stream's params. Errors:
/// - `AppError::ProcessingFailed` if no `Stream #...: Video:` line is
///   present (input has no video, or ffmpeg can't read the container).
pub fn probe_video_stream_params(path: &Path) -> Result<VideoStreamParams, AppError> {
    let bin = binary_path();
    let mut command = Command::new(&bin);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    suppress_console_window(&mut command);

    let output = command.output().map_err(|err| AppError::ProcessingFailed {
        detail: format!("failed to spawn ffmpeg at {}: {err}", bin.display()),
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_video_stream_line(&stderr).ok_or_else(|| AppError::ProcessingFailed {
        detail: format!(
            "ffmpeg -i {} did not report a Video stream line",
            path.display()
        ),
    })
}

/// Probe the first audio stream's params. Returns `Ok(None)` (NOT an
/// error) for sources with no audio — a silent video is a valid input
/// for the Video Trimmer's codec-matched re-encode.
pub fn probe_audio_stream_params(path: &Path) -> Result<Option<AudioStreamParams>, AppError> {
    let bin = binary_path();
    let mut command = Command::new(&bin);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    suppress_console_window(&mut command);

    let output = command.output().map_err(|err| AppError::ProcessingFailed {
        detail: format!("failed to spawn ffmpeg at {}: {err}", bin.display()),
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_audio_stream_line(&stderr))
}

/// Probe how many audio streams a media file declares, by parsing
/// `ffmpeg -i <path>`'s stderr for `Stream #...: Audio: ...` lines.
/// Returns 0 for sources with no audio (e.g. a silent video).
///
/// Same shape as [`probe_duration_secs`]: ffmpeg exits non-zero with no
/// output target, so we ignore exit status and rely on stderr alone. The
/// `Stream #N:M(...): Audio: <codec>, ...` line format has been stable
/// for as long as the `Duration:` line — both are documented in ffmpeg's
/// `dump_metadata` path and used by every container's demuxer.
pub fn probe_audio_stream_count(path: &Path) -> Result<u32, AppError> {
    let bin = binary_path();
    let mut command = Command::new(&bin);
    command
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    suppress_console_window(&mut command);

    let output = command.output().map_err(|err| AppError::ProcessingFailed {
        detail: format!("failed to spawn ffmpeg at {}: {err}", bin.display()),
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(parse_audio_stream_count(&stderr))
}

/// Parse one `-progress pipe:1` line. Returns `Some` only for the
/// `out_time_us` key. Trims a trailing `\r` so the Windows `\r\n` line
/// ending parses identically to Unix `\n`.
fn parse_progress_line(line: &str) -> Option<FfmpegProgress> {
    let line = line.trim_end_matches('\r').trim();
    let (key, value) = line.split_once('=')?;
    if key.trim() != "out_time_us" {
        return None;
    }
    let out_time_us: u64 = value.trim().parse().ok()?;
    Some(FfmpegProgress { out_time_us })
}

/// Parse the `Duration: HH:MM:SS.cc, start: …` line from ffmpeg's stderr.
/// Returns `None` if the line is absent, malformed, or `N/A`.
fn parse_duration_line(stderr: &str) -> Option<f64> {
    for line in stderr.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("Duration:") else {
            continue;
        };
        // After "Duration:" comes "HH:MM:SS.cc, start: …".
        let dur = rest.trim_start().split(',').next()?.trim();
        if dur == "N/A" {
            return None;
        }
        let mut parts = dur.split(':');
        let h: f64 = parts.next()?.parse().ok()?;
        let m: f64 = parts.next()?.parse().ok()?;
        let s: f64 = parts.next()?.parse().ok()?;
        return Some(h * 3600.0 + m * 60.0 + s);
    }
    None
}

/// Count audio streams in ffmpeg's `-i` banner output. A stream line looks
/// like `  Stream #0:1(und): Audio: aac (LC), 48000 Hz, stereo, fltp, …`
/// (the language tag in parens and a `[0xNNNN]` hex stream id may or may
/// not be present). We only need two anchor substrings on the same line:
/// the `Stream #` prefix and the `: Audio:` marker.
fn parse_audio_stream_count(stderr: &str) -> u32 {
    let mut count: u32 = 0;
    for line in stderr.lines() {
        // BufReader::lines() already handles `\n` for us in real callers;
        // the explicit trim handles raw `\r\n` strings dropped into tests.
        let trimmed = line.trim_end_matches('\r').trim();
        if trimmed.starts_with("Stream #") && trimmed.contains(": Audio:") {
            count = count.saturating_add(1);
        }
    }
    count
}

/// Parse the FIRST `Stream #...: Video: <codec> [(profile)], <pixfmt>[(color
/// tags)], <W>x<H>, …` line out of ffmpeg's stderr. Tolerates the language
/// tag in `(...)` and the mpegts hex stream id in `[0xNNNN]` (both optional)
/// before the `: Video: ` marker. Returns `None` if no video stream line is
/// present, or if the codec/dimensions slots are unparseable.
fn parse_video_stream_line(stderr: &str) -> Option<VideoStreamParams> {
    for line in stderr.lines() {
        let trimmed = line.trim_end_matches('\r').trim();
        if !trimmed.starts_with("Stream #") {
            continue;
        }
        let Some(rest) = trimmed.split(": Video:").nth(1) else {
            continue;
        };
        let fields = split_top_level_commas(rest.trim());
        let codec_field = fields.first()?;
        // First whitespace-separated token: the codec name. Anything that
        // follows in parens is the profile/level (`h264 (High)` → `h264`).
        let codec = codec_field.split_whitespace().next()?.to_ascii_lowercase();

        // Pixfmt may carry trailing `(tv, bt709, ...)` color tags — strip
        // them at the first `(`. If the second field doesn't look like a
        // pixfmt (e.g. a stream missing the slot), return None for pix_fmt.
        let pix_fmt = fields.get(1).and_then(|f| {
            let raw = f.trim();
            let head = raw.split('(').next()?.trim();
            if head.is_empty() {
                None
            } else {
                Some(head.to_string())
            }
        });

        // Dimensions slot — `1920x1080`. May land in field 2 (typical) or
        // later when an extra slot precedes it (rare). Scan for the first
        // `WxH`-shaped field.
        let (width, height) = fields.iter().skip(2).find_map(|f| {
            let (w, h) = f.trim().split_once('x')?;
            Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
        })?;

        return Some(VideoStreamParams {
            codec,
            pix_fmt,
            width,
            height,
        });
    }
    None
}

/// Parse the FIRST `Stream #...: Audio: <codec> [(profile)], <rate> Hz,
/// <layout>, …` line out of ffmpeg's stderr. Sources with no audio
/// stream return `None`. Multi-track sources return the FIRST track —
/// see the [`AudioStreamParams`] doc for the codec-mirror rationale.
fn parse_audio_stream_line(stderr: &str) -> Option<AudioStreamParams> {
    for line in stderr.lines() {
        let trimmed = line.trim_end_matches('\r').trim();
        if !trimmed.starts_with("Stream #") {
            continue;
        }
        let Some(rest) = trimmed.split(": Audio:").nth(1) else {
            continue;
        };
        let fields = split_top_level_commas(rest.trim());
        let codec_field = fields.first()?;
        let codec = codec_field.split_whitespace().next()?.to_ascii_lowercase();

        // Sample rate slot: "48000 Hz" / "44100 Hz" / "N/A".
        let sample_rate = fields.get(1).and_then(|f| {
            let raw = f.trim();
            let num = raw.strip_suffix("Hz").unwrap_or(raw).trim();
            num.parse::<u32>().ok()
        });

        let channels = fields.get(2).map(|f| f.trim().to_string());

        return Some(AudioStreamParams {
            codec,
            sample_rate,
            channels,
        });
    }
    None
}

/// Split `s` on commas at paren-depth 0. Tolerates both `(...)` (language
/// tags / color tags / profile parens) and `[...]` (mpegts hex stream id)
/// nesting. Trims whitespace around each field.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut depth: i32 = 0;
    let mut start = 0usize;
    let mut out: Vec<&str> = Vec::new();
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth = depth.saturating_add(1),
            ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(s[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(s[start..].trim());
    out
}

/// On Windows, ensure spawning a child process doesn't pop a console window
/// — the Tauri app is a GUI process and a flashing cmd.exe is jarring. No-op
/// on other platforms.
#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    // CREATE_NO_WINDOW = 0x08000000 — keeps the GUI parent from briefly
    // flashing a console host when ffmpeg starts.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_out_time_us_line() {
        let p = parse_progress_line("out_time_us=12345678").expect("parse out_time_us");
        assert_eq!(p.out_time_us, 12_345_678);
    }

    #[test]
    fn ignores_non_out_time_us_keys() {
        assert!(parse_progress_line("bitrate=128.5kbits/s").is_none());
        assert!(parse_progress_line("progress=continue").is_none());
        assert!(parse_progress_line("total_size=0").is_none());
        assert!(parse_progress_line("frame=42").is_none());
    }

    #[test]
    fn ignores_garbage_lines() {
        assert!(parse_progress_line("").is_none());
        assert!(parse_progress_line("not a key value line").is_none());
        assert!(parse_progress_line("=12345").is_none());
        assert!(parse_progress_line("out_time_us=not_an_int").is_none());
    }

    #[test]
    fn handles_windows_crlf_line_ending() {
        // BufReader::lines() already strips `\n`, so what reaches the parser
        // on Windows has only a trailing `\r`. Confirm we trim it.
        let p = parse_progress_line("out_time_us=42\r").expect("parse trailing \\r");
        assert_eq!(p.out_time_us, 42);
    }

    #[test]
    fn parses_duration_from_typical_ffmpeg_banner() {
        let stderr = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'input.mp4':\n  \
            Metadata:\n    \
            major_brand     : isom\n  \
            Duration: 00:01:23.45, start: 0.000000, bitrate: 1024 kb/s\n  \
            Stream #0:0(und): Video: h264\n";
        let secs = parse_duration_line(stderr).expect("parse duration");
        // 1m 23.45s = 83.45s
        assert!((secs - 83.45).abs() < 0.01, "expected ~83.45, got {secs}");
    }

    #[test]
    fn duration_n_a_returns_none() {
        let stderr = "  Duration: N/A, bitrate: N/A\n";
        assert!(parse_duration_line(stderr).is_none());
    }

    #[test]
    fn duration_missing_returns_none() {
        let stderr = "Input #0, mp4: nope\nStream #0:0: Video: h264\n";
        assert!(parse_duration_line(stderr).is_none());
    }

    #[test]
    fn duration_handles_subsecond_precision_and_long_runs() {
        let stderr = "  Duration: 10:30:45.99, start: 0.000000\n";
        let secs = parse_duration_line(stderr).expect("parse long duration");
        // 10h 30m 45.99s = 37845.99s
        assert!(
            (secs - 37_845.99).abs() < 0.01,
            "expected ~37845.99, got {secs}"
        );
    }

    #[test]
    fn audio_count_zero_for_video_only_banner() {
        let stderr = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'silent.mp4':\n  \
            Duration: 00:00:05.00, start: 0.000000, bitrate: 256 kb/s\n  \
            Stream #0:0(und): Video: h264 (High), yuv420p, 1280x720, 256 kb/s\n";
        assert_eq!(parse_audio_stream_count(stderr), 0);
    }

    #[test]
    fn audio_count_one_for_standard_video_with_audio() {
        let stderr = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'clip.mp4':\n  \
            Duration: 00:00:10.00, start: 0.000000, bitrate: 1024 kb/s\n  \
            Stream #0:0(und): Video: h264 (High), yuv420p, 1280x720\n  \
            Stream #0:1(und): Audio: aac (LC), 48000 Hz, stereo, fltp, 128 kb/s\n";
        assert_eq!(parse_audio_stream_count(stderr), 1);
    }

    #[test]
    fn audio_count_handles_multiple_audio_tracks() {
        // mkv with three audio tracks (typical: original + dub + commentary).
        let stderr = "Input #0, matroska,webm, from 'movie.mkv':\n  \
            Duration: 01:30:00.00, bitrate: 8000 kb/s\n  \
            Stream #0:0(eng): Video: h264 (High), yuv420p, 1920x1080\n  \
            Stream #0:1(eng): Audio: ac3, 48000 Hz, 5.1, fltp, 448 kb/s\n  \
            Stream #0:2(jpn): Audio: aac, 48000 Hz, stereo, fltp, 160 kb/s\n  \
            Stream #0:3(eng): Audio: aac, 48000 Hz, stereo, fltp, 96 kb/s\n  \
            Stream #0:4(eng): Subtitle: subrip\n";
        assert_eq!(parse_audio_stream_count(stderr), 3);
    }

    #[test]
    fn audio_count_handles_hex_stream_id_in_bracket() {
        // ts/mts containers print a `[0xNNNN]` hex stream id between the
        // index and the language tag. Anchor substrings still match.
        let stderr = "Input #0, mpegts, from 'broadcast.ts':\n  \
            Duration: 00:30:00.00\n  \
            Stream #0:0[0x100]: Video: h264, yuv420p, 1920x1080\n  \
            Stream #0:1[0x101](eng): Audio: ac3, 48000 Hz, 5.1\n";
        assert_eq!(parse_audio_stream_count(stderr), 1);
    }

    #[test]
    fn audio_count_ignores_non_stream_audio_mentions() {
        // The word "Audio" can appear in banner text outside a Stream line.
        // We anchor on BOTH `Stream #` and `: Audio:` so prose isn't counted.
        let stderr = "Input #0, mp4, from 'x.mp4':\n  \
            Metadata:\n    \
            handler_name    : SoundHandler (Audio mixed by …)\n  \
            Stream #0:0: Video: h264\n";
        assert_eq!(parse_audio_stream_count(stderr), 0);
    }

    #[test]
    fn audio_count_handles_windows_crlf() {
        let stderr = "  Stream #0:0(und): Video: h264, yuv420p\r\n  \
             Stream #0:1(und): Audio: aac (LC), 48000 Hz, stereo\r\n";
        assert_eq!(parse_audio_stream_count(stderr), 1);
    }

    #[test]
    fn audio_count_zero_on_empty_input() {
        assert_eq!(parse_audio_stream_count(""), 0);
    }

    #[test]
    fn parses_video_line_h264_yuv420p() {
        let stderr = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'clip.mp4':\n  \
            Duration: 00:00:10.00, start: 0.000000, bitrate: 1024 kb/s\n  \
            Stream #0:0(und): Video: h264 (High), yuv420p, 1280x720, 1024 kb/s, 30 fps\n  \
            Stream #0:1(und): Audio: aac (LC), 48000 Hz, stereo, fltp, 128 kb/s\n";
        let params = parse_video_stream_line(stderr).expect("parse video");
        assert_eq!(params.codec, "h264");
        assert_eq!(params.pix_fmt.as_deref(), Some("yuv420p"));
        assert_eq!(params.width, 1280);
        assert_eq!(params.height, 720);
    }

    #[test]
    fn parses_video_line_hevc_10bit_with_hdr_color_tags() {
        // 10-bit pixfmt + HDR color metadata in parens. The paren-aware
        // splitter must not break the pixfmt slot on the embedded comma.
        let stderr = "  Stream #0:0(eng): Video: hevc (Main 10), \
            yuv420p10le(tv, bt2020nc/bt2020/smpte2084), 3840x2160, 15000 kb/s\n";
        let params = parse_video_stream_line(stderr).expect("parse hevc 10-bit");
        assert_eq!(params.codec, "hevc");
        assert_eq!(params.pix_fmt.as_deref(), Some("yuv420p10le"));
        assert_eq!(params.width, 3840);
        assert_eq!(params.height, 2160);
    }

    #[test]
    fn parses_video_line_vp9_no_profile() {
        let stderr = "  Stream #0:0(und): Video: vp9, yuv420p(tv, bt709), 1920x1080, 25 fps\n";
        let params = parse_video_stream_line(stderr).expect("parse vp9");
        assert_eq!(params.codec, "vp9");
        assert_eq!(params.pix_fmt.as_deref(), Some("yuv420p"));
        assert_eq!(params.width, 1920);
        assert_eq!(params.height, 1080);
    }

    #[test]
    fn parses_video_line_av1() {
        let stderr = "  Stream #0:0: Video: av1 (Main), yuv420p, 1920x1080, 30 fps\n";
        let params = parse_video_stream_line(stderr).expect("parse av1");
        assert_eq!(params.codec, "av1");
        assert_eq!(params.pix_fmt.as_deref(), Some("yuv420p"));
    }

    #[test]
    fn parses_video_line_handles_mpegts_hex_stream_id() {
        let stderr = "  Stream #0:0[0x100]: Video: h264, yuv420p, 1920x1080, 8000 kb/s\n";
        let params = parse_video_stream_line(stderr).expect("parse mpegts h264");
        assert_eq!(params.codec, "h264");
        assert_eq!(params.width, 1920);
        assert_eq!(params.height, 1080);
    }

    #[test]
    fn parses_video_line_uppercase_codec_name_normalized() {
        // mxf / wmv banners can print the codec name in mixed case (e.g.
        // `MPEG2VIDEO`). The mirror table keys on lowercase ffmpeg names.
        let stderr = "  Stream #0:0: Video: MPEG2VIDEO, yuv420p, 720x576\n";
        let params = parse_video_stream_line(stderr).expect("parse mpeg2");
        assert_eq!(params.codec, "mpeg2video");
    }

    #[test]
    fn video_line_missing_returns_none() {
        assert!(parse_video_stream_line("").is_none());
        // Audio-only file — no video stream line.
        assert!(parse_video_stream_line(
            "Input #0, mp3, from 'song.mp3':\n  \
                Duration: 00:03:00.00\n  \
                Stream #0:0: Audio: mp3, 44100 Hz, stereo\n",
        )
        .is_none());
    }

    #[test]
    fn video_line_handles_crlf() {
        let stderr = "  Stream #0:0(und): Video: h264, yuv420p, 1280x720\r\n";
        let params = parse_video_stream_line(stderr).expect("parse with crlf");
        assert_eq!(params.codec, "h264");
    }

    #[test]
    fn parses_audio_line_aac_stereo() {
        let stderr = "  Stream #0:0(und): Video: h264, yuv420p, 1280x720\n  \
            Stream #0:1(und): Audio: aac (LC), 48000 Hz, stereo, fltp, 128 kb/s\n";
        let params = parse_audio_stream_line(stderr).expect("parse aac");
        assert_eq!(params.codec, "aac");
        assert_eq!(params.sample_rate, Some(48_000));
        assert_eq!(params.channels.as_deref(), Some("stereo"));
    }

    #[test]
    fn parses_audio_line_opus_without_bitrate_slot() {
        // opus / flac frequently omit the trailing `, N kb/s` slot.
        let stderr = "  Stream #0:1: Audio: opus, 48000 Hz, stereo, fltp\n";
        let params = parse_audio_stream_line(stderr).expect("parse opus");
        assert_eq!(params.codec, "opus");
        assert_eq!(params.sample_rate, Some(48_000));
        assert_eq!(params.channels.as_deref(), Some("stereo"));
    }

    #[test]
    fn parses_audio_line_flac_lossless() {
        let stderr = "  Stream #0:1: Audio: flac, 96000 Hz, 5.1, s32 (24 bit)\n";
        let params = parse_audio_stream_line(stderr).expect("parse flac");
        assert_eq!(params.codec, "flac");
        assert_eq!(params.sample_rate, Some(96_000));
        assert_eq!(params.channels.as_deref(), Some("5.1"));
    }

    #[test]
    fn audio_line_takes_first_track_when_multiple_present() {
        let stderr = "  Stream #0:0(eng): Video: h264, yuv420p, 1920x1080\n  \
            Stream #0:1(eng): Audio: ac3, 48000 Hz, 5.1, fltp, 448 kb/s\n  \
            Stream #0:2(jpn): Audio: aac, 48000 Hz, stereo, fltp, 160 kb/s\n";
        let params = parse_audio_stream_line(stderr).expect("parse first audio");
        assert_eq!(params.codec, "ac3");
        assert_eq!(params.channels.as_deref(), Some("5.1"));
    }

    #[test]
    fn audio_line_absent_returns_none() {
        let stderr = "  Stream #0:0: Video: h264, yuv420p, 1280x720\n";
        assert!(parse_audio_stream_line(stderr).is_none());
    }

    #[test]
    fn audio_line_handles_mpegts_hex_stream_id() {
        let stderr = "  Stream #0:1[0x101](eng): Audio: ac3, 48000 Hz, 5.1, fltp\n";
        let params = parse_audio_stream_line(stderr).expect("parse mpegts audio");
        assert_eq!(params.codec, "ac3");
        assert_eq!(params.channels.as_deref(), Some("5.1"));
    }

    #[test]
    fn audio_line_handles_crlf() {
        let stderr = "  Stream #0:1(und): Audio: aac, 48000 Hz, stereo\r\n";
        let params = parse_audio_stream_line(stderr).expect("parse with crlf");
        assert_eq!(params.codec, "aac");
    }

    #[test]
    fn split_top_level_commas_respects_paren_depth() {
        // Color tags inside the pixfmt parens must NOT cause a split.
        let fields = split_top_level_commas(
            "hevc (Main 10), yuv420p10le(tv, bt2020nc/bt2020), 3840x2160, 15000 kb/s",
        );
        assert_eq!(
            fields,
            vec![
                "hevc (Main 10)",
                "yuv420p10le(tv, bt2020nc/bt2020)",
                "3840x2160",
                "15000 kb/s",
            ]
        );
    }

    #[test]
    fn split_top_level_commas_respects_bracket_depth() {
        // mpegts `[0xNNNN]` brackets — same rule.
        let fields = split_top_level_commas("h264 [0x100, 0x200], yuv420p, 1280x720");
        assert_eq!(fields, vec!["h264 [0x100, 0x200]", "yuv420p", "1280x720"]);
    }
}
