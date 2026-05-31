//! Codec-mirror tables for re-encoding video tools.
//!
//! Shared primitives both video tools call when building an ffmpeg arg
//! vec. The Video Trimmer always re-encodes at each codec's "transparent
//! for one generation" CRF (the `0` slot of [`video_crf_range`]); the
//! Video Compressor interpolates a CRF between the two slots from a
//! quality slider. Audio uses a fixed mirror — the trimmer re-encodes
//! at a per-codec perceptually-transparent bitrate, the compressor
//! stream-copies in v1 (the audio backlog item will consume this when
//! audio scaling lands).
//!
//! VP8 carries a fixed `-b:v 1M` cap regardless of the CRF passed — VP8
//! needs both flags for stable quality and only the trimmer relies on
//! VP8 in v1; if the compressor ever wants VP8 with a scaling bitrate
//! cap, add a dedicated builder rather than overloading this one.

/// Per-codec CRF range, `(best, worst)`: smaller CRF = higher quality.
/// The `best` value is what the Video Trimmer treats as perceptually
/// transparent for a single re-encode, and is also the `slider=1.0`
/// anchor for the Video Compressor; `worst` is the
/// smallest-file-still-watchable bound for `slider=0.0`. Unknown source
/// codecs fall back to the H.264 range — same fallback the encoder
/// table uses.
pub fn video_crf_range(codec: &str) -> (u32, u32) {
    match codec {
        "h264" => (18, 32),
        "hevc" => (20, 34),
        "vp9" => (28, 40),
        "av1" => (30, 42),
        "vp8" => (10, 25),
        _ => (18, 32),
    }
}

/// Encoder + quality flags for a source video codec name, parameterised
/// by `crf`. Anything we don't recognise falls back to libx264 — it's
/// universally playable, ships in every release of the bundled binary,
/// and lands smaller than the source for most legacy codecs.
pub fn video_encoder_flags(codec: &str, crf: u32) -> Vec<String> {
    let crf = crf.to_string();
    match codec {
        "h264" => vec![
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            crf,
            "-preset".into(),
            "medium".into(),
        ],
        "hevc" => vec![
            "-c:v".into(),
            "libx265".into(),
            "-crf".into(),
            crf,
            "-preset".into(),
            "medium".into(),
        ],
        "vp9" => vec![
            "-c:v".into(),
            "libvpx-vp9".into(),
            "-crf".into(),
            crf,
            "-b:v".into(),
            "0".into(),
            "-row-mt".into(),
            "1".into(),
        ],
        "av1" => vec![
            "-c:v".into(),
            "libaom-av1".into(),
            "-crf".into(),
            crf,
            "-cpu-used".into(),
            "6".into(),
            "-row-mt".into(),
            "1".into(),
        ],
        "vp8" => vec![
            "-c:v".into(),
            "libvpx".into(),
            "-crf".into(),
            crf,
            "-b:v".into(),
            "1M".into(),
        ],
        _ => vec![
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            crf,
            "-preset".into(),
            "medium".into(),
        ],
    }
}

/// Encoder + bitrate flags for a source audio codec name. AAC is the
/// universal fallback — it's the lingua franca of mp4/mov containers and
/// rounds out anything mp3-or-older the table doesn't list.
///
/// Per-codec defaults aim at perceptually transparent for a single
/// re-encode: AAC 128k (stereo music transparent; avoids the 4×
/// over-encode against low-bitrate phone-capture sources that smoke-1
/// caught on the trimmer); AC3 192k (typically surround, cutting further
/// degrades the mix).
pub fn audio_encoder_flags(codec: &str) -> &'static [&'static str] {
    match codec {
        "aac" => &["-c:a", "aac", "-b:a", "128k"],
        "opus" => &["-c:a", "libopus", "-b:a", "128k"],
        "mp3" => &["-c:a", "libmp3lame", "-q:a", "2"],
        "flac" => &["-c:a", "flac"],
        "vorbis" => &["-c:a", "libvorbis", "-q:a", "5"],
        "ac3" => &["-c:a", "ac3", "-b:a", "192k"],
        _ => &["-c:a", "aac", "-b:a", "128k"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_crf_range_best_matches_trimmer_transparent_crfs() {
        // The `best` slot is load-bearing: it's both the trimmer's
        // historical transparent CRF and the compressor's slider=1.0
        // anchor. Pinning these guards against accidental tweaks.
        assert_eq!(video_crf_range("h264").0, 18);
        assert_eq!(video_crf_range("hevc").0, 20);
        assert_eq!(video_crf_range("vp9").0, 28);
        assert_eq!(video_crf_range("av1").0, 30);
        assert_eq!(video_crf_range("vp8").0, 10);
    }

    #[test]
    fn video_crf_range_worst_is_strictly_greater_than_best() {
        // Compressor lerps worst → best as the slider moves 0 → 1.
        // If worst <= best, the slider has no effect on size.
        for codec in ["h264", "hevc", "vp9", "av1", "vp8"] {
            let (best, worst) = video_crf_range(codec);
            assert!(
                worst > best,
                "{codec} needs a worst > best (got {best}/{worst})"
            );
        }
    }

    #[test]
    fn video_crf_range_unknown_codec_falls_back_to_h264() {
        for exotic in ["mpeg2video", "wmv3", "mpeg4", "prores", ""] {
            assert_eq!(
                video_crf_range(exotic),
                video_crf_range("h264"),
                "fallback for {exotic}"
            );
        }
    }

    #[test]
    fn video_encoder_flags_map_known_codecs_to_same_family_encoders() {
        let h264 = video_encoder_flags("h264", 18);
        assert_eq!(h264[0], "-c:v");
        assert_eq!(h264[1], "libx264");
        assert!(h264.iter().any(|s| s == "-crf"));

        let hevc = video_encoder_flags("hevc", 20);
        assert_eq!(hevc[1], "libx265");

        let vp9 = video_encoder_flags("vp9", 28);
        assert_eq!(vp9[1], "libvpx-vp9");
        assert!(
            vp9.iter().any(|s| s == "-row-mt"),
            "vp9 needs row-mt for parallelism"
        );

        let av1 = video_encoder_flags("av1", 30);
        assert_eq!(av1[1], "libaom-av1");

        let vp8 = video_encoder_flags("vp8", 10);
        assert_eq!(vp8[1], "libvpx");
        let bv = vp8.iter().position(|s| s == "-b:v").unwrap();
        assert_eq!(
            vp8[bv + 1],
            "1M",
            "VP8 carries a fixed bitrate cap regardless of CRF"
        );
    }

    #[test]
    fn video_encoder_flags_unknown_codec_falls_back_to_libx264() {
        for exotic in ["mpeg2video", "wmv3", "mpeg4", "prores", "dvvideo", ""] {
            let flags = video_encoder_flags(exotic, 18);
            assert_eq!(flags[1], "libx264", "fallback for {exotic}");
        }
    }

    #[test]
    fn video_encoder_flags_carries_supplied_crf() {
        // Every CRF-carrying codec drops `-crf <n>` somewhere in the vec.
        for codec in ["h264", "hevc", "vp9", "av1", "vp8"] {
            for crf in [4u32, 18, 28, 40] {
                let flags = video_encoder_flags(codec, crf);
                let crf_idx = flags
                    .iter()
                    .position(|s| s == "-crf")
                    .unwrap_or_else(|| panic!("{codec} missing -crf"));
                assert_eq!(
                    flags[crf_idx + 1],
                    crf.to_string(),
                    "{codec} should carry CRF {crf} exactly"
                );
            }
        }
    }

    #[test]
    fn audio_encoder_flags_map_known_codecs() {
        assert_eq!(audio_encoder_flags("aac")[1], "aac");
        assert_eq!(audio_encoder_flags("opus")[1], "libopus");
        assert_eq!(audio_encoder_flags("mp3")[1], "libmp3lame");
        assert_eq!(audio_encoder_flags("flac")[1], "flac");
        assert_eq!(audio_encoder_flags("vorbis")[1], "libvorbis");
        assert_eq!(audio_encoder_flags("ac3")[1], "ac3");
    }

    #[test]
    fn audio_encoder_flags_unknown_codec_falls_back_to_aac() {
        for exotic in ["pcm_s16le", "wmav2", "dts", ""] {
            let flags = audio_encoder_flags(exotic);
            assert_eq!(flags[1], "aac", "fallback for {exotic}");
        }
    }
}
