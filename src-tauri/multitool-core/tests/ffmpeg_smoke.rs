//! Spike test: spawn the bundled ffmpeg binary end-to-end.
//!
//! Verifies the commit #1 build pipeline + commit #2 shim work together
//! on every supported target: the binary was downloaded into `OUT_DIR`,
//! `ffmpeg::run` can spawn it, the `-progress pipe:1` parser fires
//! callbacks, and `probe_duration_secs` parses the ffmpeg banner.
//!
//! Must pass on linux / macos / windows in CI. If this test fails locally
//! after a fresh checkout, the most likely cause is the build script not
//! having had a chance to download yet — try `cargo build -p multitool-core`
//! first.

use std::sync::{Arc, Mutex};

use multitool_core::ffmpeg;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

/// 1-second synthetic clip via the `lavfi` virtual device. Ultra-fast preset
/// so the encode finishes in well under a second on any builder.
fn synth_clip_args(out: &str) -> [&str; 9] {
    [
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=1:size=64x64:rate=10",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        out,
    ]
}

#[test]
fn run_encodes_synth_clip_and_streams_progress() {
    let tmp = TempDir::new().expect("create tempdir");
    let out_path = tmp.path().join("out.mp4");
    let out_str = out_path.to_str().expect("utf-8 tempdir path");

    let progress = Arc::new(Mutex::new(Vec::<ffmpeg::FfmpegProgress>::new()));
    let progress_for_cb = Arc::clone(&progress);
    let cancel = CancellationToken::new();

    ffmpeg::run(
        synth_clip_args(out_str),
        |p| progress_for_cb.lock().unwrap().push(p),
        &cancel,
    )
    .expect("ffmpeg run encodes synth clip");

    assert!(out_path.exists(), "output file written");
    assert!(
        std::fs::metadata(&out_path).unwrap().len() > 0,
        "output file non-empty"
    );

    // We can't pin the exact number of progress callbacks (depends on
    // encoder speed + throttle interaction), but at least the final
    // out_time_us emission should be near 1 second of media time.
    let samples = progress.lock().unwrap();
    let last = samples.last().copied().expect("at least one progress emit");
    assert!(
        last.out_time_us >= 900_000,
        "final out_time_us {} should be ~1s",
        last.out_time_us
    );
}

#[test]
fn probe_duration_reads_synth_clip_banner() {
    let tmp = TempDir::new().expect("create tempdir");
    let out_path = tmp.path().join("clip.mp4");
    let out_str = out_path.to_str().expect("utf-8 tempdir path");

    ffmpeg::run(synth_clip_args(out_str), |_| {}, &CancellationToken::new())
        .expect("encode synth clip");

    let secs = ffmpeg::probe_duration_secs(&out_path).expect("probe duration");
    assert!((0.9..=1.1).contains(&secs), "duration {secs} should be ~1s");
}

#[test]
fn cancel_kills_in_flight_encode() {
    let tmp = TempDir::new().expect("create tempdir");
    let out_path = tmp.path().join("long.mp4");
    let out_str = out_path.to_str().expect("utf-8 tempdir path");

    // Long synth clip so the encode is still running when we cancel.
    // 600 seconds of testsrc at 30fps is plenty of work to keep ffmpeg
    // busy past the cancel signal.
    let args = [
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=600:size=256x256:rate=30",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        out_str,
    ];

    let cancel = CancellationToken::new();
    let cancel_for_cb = cancel.clone();

    let err = ffmpeg::run(
        args,
        move |_p| {
            // Fire cancel after the first progress event proves ffmpeg
            // actually started encoding. Avoids racing the spawn.
            cancel_for_cb.cancel();
        },
        &cancel,
    )
    .expect_err("run returns Err on cancel");

    assert!(
        matches!(err, multitool_core::AppError::Cancelled),
        "expected Cancelled, got {err:?}"
    );
}
