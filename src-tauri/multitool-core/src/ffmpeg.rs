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
//! 3. **[`probe_duration_secs`].** Invokes `ffmpeg -i <path>` and parses the
//!    `Duration:` line from stderr. No `ffprobe` is bundled (saves ~50–80 MB
//!    per installed bundle); the `Duration:` line format has been stable in
//!    ffmpeg for over a decade.
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
}
