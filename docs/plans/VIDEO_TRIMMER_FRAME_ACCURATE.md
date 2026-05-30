# Tool: Video Trimmer — frame-accurate re-engine

> Ephemeral working doc. Built brief → plan, kept live per commit, deleted when the
> change ships. **Not a new tool** — this re-engines the existing Video Trimmer
> ([`src-tauri/multitool-core/src/tools/video_trimmer/`](../../src-tauri/multitool-core/src/tools/video_trimmer/),
> [`src/tools/video-trimmer/`](../../src/tools/video-trimmer/)). The UI, naming,
> picker, preview proxy, and command surface all stay; only the conversion engine
> changes from `-c copy` to a codec-matched full re-encode.
>
> Follows the resolution in [BACKLOG.md → "Frame-accurate trim (full re-encode)"](BACKLOG.md).

## Summary

Replace the keyframe-snapped stream-copy with a **frame-accurate full re-encode**: ffmpeg
decodes the source, cuts at the exact requested start/end frame (output-seek with `-ss` after
`-i`), and re-encodes with parameters mirrored from the source (codec, pixel format, audio
codec) at a perceptually-transparent CRF. Always on, not a setting — accuracy beats speed,
and a learning-project trimmer doesn't have a "speed matters" user.

## Decisions locked with the user (2026-05-30)

- **Engine: full re-encode, codec-matched.** Single ffmpeg invocation; output-seek
  (`-ss` *after* `-i`) so the cut lands on the exact requested frame; encoder chosen by
  parsing the source's video/audio codec from the ffmpeg banner.
- **Partial re-encode (smart cut) is not pursued and not in the backlog.** It scores
  better on quality and file size in theory, but exact SPS/PPS matching across head/tail
  re-encodes + middle stream-copy + concat-demuxer splice is substantially more code, with
  container-dependent footguns (mp4/mkv/mov concat differs) and subtle splice-glitch failure
  modes. With no "speed matters" pressure and short typical inputs, a full re-encode at a
  matched CRF gets us frame accuracy + visually transparent quality at a fraction of the
  implementation cost. Revisit only if real usage shows the generation-loss is unacceptable.
- **Codec mirror table** (parsed from `ffmpeg -i` banner):
  | Source video codec | Encoder | Quality flag |
  | --- | --- | --- |
  | h264 | `libx264` | `-crf 18 -preset medium` |
  | hevc / h265 | `libx265` | `-crf 20 -preset medium` |
  | vp9 | `libvpx-vp9` | `-crf 28 -b:v 0 -row-mt 1` |
  | av1 | `libaom-av1` | `-crf 30 -cpu-used 6 -row-mt 1` |
  | vp8 | `libvpx` | `-crf 10 -b:v 1M` |
  | mpeg4 / mpeg2video / wmv / anything else | `libx264` fallback | `-crf 18 -preset medium` |

  CRF values are perceptually-transparent for one encoding generation. Source bitrate is
  *not* used as a target — CBR-to-match-source is a footgun (CRF can land smaller *or*
  larger; we accept the rare "output larger than source" case rather than degrade quality
  with `-maxrate`).
- **Pixel format mirrored** (`-pix_fmt` set from probe: yuv420p / yuv420p10le / yuv444p /
  …). Preserving 10-bit pixfmt matters for HDR / high-end captures — defaulting to yuv420p
  would tank quality on a 10-bit source. If the probe can't read pixfmt, fall back to
  `yuv420p` (universal compatibility).
- **Audio codec mirrored** similarly:
  | Source audio codec | Encoder | Quality flag |
  | --- | --- | --- |
  | aac | `aac` | `-b:a 192k` |
  | opus | `libopus` | `-b:a 128k` |
  | mp3 | `libmp3lame` | `-q:a 2` |
  | flac | `flac` | (lossless, no flag) |
  | vorbis | `libvorbis` | `-q:a 5` |
  | ac3 | `ac3` | `-b:a 192k` |
  | anything else | `aac` fallback | `-b:a 192k` |

  Source bitrate isn't mirrored exactly (banners often omit it for opus/flac); the table
  uses a one-step-up-from-typical default per codec.
- **Container preserved.** Same as v1 — the output extension matches the source extension.
  ffmpeg picks the muxer from the extension; mp4/mkv/mov/webm all work as round-trip targets
  for the matched codecs above.
- **Frame rate: not explicitly set.** ffmpeg defaults to source fps; VFR sources stay VFR
  unless the codec/container demands CFR (rare for the formats we accept).
- **HDR / color metadata: best-effort, not v1-blocking.** We don't currently parse
  `bt2020`/`smpte2084` from the banner. HDR sources will round-trip as SDR-tagged after
  re-encode, which is a visible quality regression on HDR displays. Acceptable for a
  learning-project v1; note in the ship-time DECISIONS entry and add to the backlog as a
  follow-up.
- **Subtitle / data / attachment streams: still dropped.** Same as v1.
- **Preview / output mismatch: gone.** Frame-accurate encode means the previewed start frame
  *is* the actual output start frame. The "may differ slightly" caveat in the component
  goes away (one comment + the user-facing copy if any).

## Inputs

Unchanged from v1 — same picker, same extension allow-list, same `pickVideoFile()` from
[`src/lib/system.ts`](../../src/lib/system.ts).

## Options

Unchanged from v1:

| Option | Type | Default | Notes |
| --- | --- | --- | --- |
| `start_ms` | u64 | 0 | Start of trim window, ms from source start. |
| `end_ms` | u64 | source duration | End of window. Clamped to probed duration; `start_ms >= end_ms` post-clamp → `ProcessingFailed`. |

No fades, no codec knobs, no quality slider — codec + CRF derive from the source.

## ffmpeg recipe

```
-y -i <input> -ss <start_s> -t <dur_s> \
  -map 0:v:0 -map 0:a? \
  -c:v <matched_video_codec> [<matched_quality_flags>] -pix_fmt <matched_pix_fmt> \
  -c:a <matched_audio_codec> [<matched_audio_flags>] \
  -avoid_negative_ts make_zero \
  <output>
```

Key shape changes from v1's `-c copy` invocation:

- **`-ss` AFTER `-i`** (output seek). Decodes from the previous keyframe and discards
  frames until the requested start — frame-accurate by construction. Slower than the v1
  input seek, but accuracy is the whole point.
- **`-map 0:v:0`** (single video stream, no `?`). A re-encode can't accept "all video
  streams" the way `-c copy` could — multi-video-stream sources are vanishingly rare for
  our input set; pick stream 0 explicitly.
- **`-map 0:a?`** kept optional — silent sources still work.
- **`-c:v <encoder>` + quality flags from the mirror table** (see Decisions).
- **`-pix_fmt <mirrored>`**, falling back to `yuv420p` only if the probe can't read it.
- **`-c:a <encoder>` + audio flags from the mirror table.**
- **`-avoid_negative_ts make_zero`** kept (defends against quirky source timestamps even
  on a re-encode path).
- The standard `-progress pipe:1 -nostats -hide_banner -loglevel error` prefix is prepended
  by [`crate::ffmpeg::run`](../../src-tauri/multitool-core/src/ffmpeg.rs); don't pass it.

## Output

Unchanged from v1:

- **Location:** same directory as the input.
- **Naming:** `{stem}_trimmed.{ext}` via [`derive_output_path`](../../src-tauri/multitool-core/src/tools/video_trimmer/convert.rs).
- **Duplicate handling:** `multitool_core::fs::unique_path` → `{stem}_trimmed (1).{ext}`.
- **Partial cleanup on error / cancel:** unlinked, same as v1.

## UX flow

Unchanged from v1:

- Dashboard → Video Trimmer → pick file → picked view → Trim → done.
- State machine: `idle → loading → preparing-proxy → picked → running → done | error`.
- Progress: the running state now lasts visibly longer (re-encode beats stream-copy by
  orders of magnitude). The existing progress bar drives off `out_time_us / trim_duration`
  and works as-is — no UI change needed beyond removing the "may differ slightly" comment.
- Cancellation: still aborts via signal → `cancel_job` → ffmpeg child killed, partial
  removed.

## Shared-surface touches

The pure-logic side picks up one new helper; the rest of the surface is per-tool.

- **`multitool_core::ffmpeg::probe_video_stream_params`** (new) — parses the `Stream
  #N:M: Video: <codec> (<profile>), <pixfmt>(<color tags>), <WxH>, <bitrate>` line and
  returns a struct `{ codec: String, pix_fmt: Option<String>, width: u32, height: u32 }`.
  Same shape as the existing [`probe_audio_stream_count`](../../src-tauri/multitool-core/src/ffmpeg.rs) — re-uses `Command::new(bin).arg("-i").arg(path)`,
  parses stderr, has its own unit tests on the parser against typical/edge banner shapes.
- **`multitool_core::ffmpeg::probe_audio_stream_params`** (new) — parses the first
  `Stream #N:M: Audio: <codec> (<profile>), <rate> Hz, <layout>, <fmt>, <bitrate>` line
  and returns `{ codec: String, sample_rate: Option<u32>, channels: Option<String> }`.
  Audio bitrate is not used (banners frequently omit it); the per-codec defaults in the
  mirror table cover everything we ship.
- **Codec → encoder mapping** lives in the tool's `convert.rs` (`video_codec_args`,
  `audio_codec_args` static slices), not in the ffmpeg shim. Shim stays codec-agnostic.

No frontend wrapper changes, no command-surface changes. The shell `trim_video` command is
unaffected — same opts in, same `JobResult` out.

## Edge cases

- `start_ms >= end_ms` after clamping → `ProcessingFailed` (unchanged).
- `end_ms` beyond source duration → clamped silently (unchanged).
- Source unreadable → ffmpeg non-zero exit → `ProcessingFailed` (unchanged).
- Missing input → `FileNotFound` (unchanged).
- **No keyframe-snap caveat.** Output starts at the exact requested frame.
- **10-bit source (yuv420p10le, yuv422p10le, …)** preserved via `-pix_fmt` mirror;
  fallback yuv420p only when the probe can't parse pixfmt.
- **HDR source** re-encoded as SDR-tagged (known v1 limitation; backlog follow-up).
- **Multi-video-stream source** (rare; e.g. an mkv with a thumbnail track typed as video):
  only stream 0 is mapped. Acceptable; trimmers don't typically need a thumbnail track.
- **Multiple audio tracks** all preserved (`-map 0:a?`) but encoded with the same codec
  derived from track 0's codec. Acceptable for a learning-project trimmer.
- **VFR source** stays VFR (no explicit `-r` set).
- **Output file may be larger than input** for very efficiently-encoded sources at low
  CRF; rare in practice, not flagged as an error. The matching params requirement is "use
  comparable codec settings so the output is in the same ballpark", not "output ≤ input
  always".
- **Encoder not in the bundled ffmpeg.** Our build includes libx264 / libx265 / libvpx /
  libvpx-vp9 / libsvtav1 / aac / libmp3lame / libopus / libvorbis / flac / ac3 (confirm at
  step 1 below). If a needed encoder is missing, swap the mapping to the libx264 / aac
  fallback rather than failing.

## Acceptance

- [ ] Pick a long-GOP video, trim a sub-range → output starts at the exact selected frame
      (no keyframe snap-back), ends at the exact selected frame.
- [ ] Output container matches source; video codec matches source family (h264-in →
      h264-out, hevc-in → hevc-out, vp9-in → vp9-out).
- [ ] Re-encoded clip is visually indistinguishable from the source at typical viewing
      distance (perceptually transparent CRF).
- [ ] Invalid range rejected with no file written; collision lands `(1)`; cancel removes
      the partial; missing input → `FileNotFound`.
- [ ] Rust ≥80% on `convert.rs` and on the new probe parsers; existing wrapper +
      component Vitest still passes (no IPC-shape change); existing Playwright happy-path
      still passes.

---

## Commit plan (live — flip to done + SHA + gotcha per commit)

| # | Commit | Status |
| --- | --- | --- |
| 1 | `docs(video-trimmer): brief for frame-accurate re-engine` | pending |
| 2 | `feat(ffmpeg): probe_video_stream_params + probe_audio_stream_params + parser tests` | pending |
| 3 | `feat(video-trimmer): codec-mirror tables + frame-accurate args (convert.rs)` | pending |
| 4 | `feat(video-trimmer): wire probe → convert (codec-matched re-encode end-to-end)` | pending |
| — | **STOP — manual smoke session #1** (long-GOP h264 mp4, hevc mp4, vp9 webm; cancel mid-encode; invalid-range; multi-audio-track mkv) | — |
| 5 | `fix(video-trimmer): smoke fixes (TBD per session)` | pending |
| 6 | `test(video-trimmer): refresh Playwright happy-path expectations` (drop keyframe-snap caveat copy if present) | pending |
| 7 | `chore(video-trimmer): ship — DECISIONS entry, drop working doc` | pending |

### Step 1 prerequisite — confirm bundled-ffmpeg encoder coverage

Linux check ran 2026-05-30 against `target/release/resources/ffmpeg/ffmpeg` (eugeneware ffmpeg-static b6.1.1):

- ✅ Present: libx264, libx265, libvpx, libvpx-vp9, aac, ac3, libmp3lame, libopus, libvorbis, flac.
- ❌ Missing: `libsvtav1`. Bundled binary ships `libaom-av1` instead (slower, similar quality
  at higher CRF). Mirror table updated above to use `libaom-av1` for av1 sources.

CI is the cross-OS truth — re-run on Windows/macOS during the first CI sweep (commit 2 or 3)
to confirm the eugeneware bundle ships the same encoder set on those targets. If it doesn't,
narrow the table to the intersection and rely on the libx264 / aac fallback. Record the final
coverage in the DECISIONS ship-entry.

### Open sub-calls

- **HDR pass-through** — out of scope for v1, but worth a 30-line look at whether
  `-color_primaries`/`-color_trc`/`-colorspace` can be mirrored from the banner cheaply
  before committing it to the backlog as a follow-up. Decide during commit 2.
- **Two-pass for very short trims** — a re-encode at the head of a long source needs to
  decode from the previous keyframe to the cut point, which can be slow for long-GOP
  sources. ffmpeg's output-seek handles this automatically; no two-pass is needed. Noted
  here so a future contributor doesn't reinvent it.

### Manual long-GOP test material (for smoke #1)

- OBS recording of a 30s screen capture — default GOP ~250 frames (~8s @ 30fps); the
  documented worst case for v1's keyframe-snap caveat.
- Synthesized: `ffmpeg -f lavfi -i testsrc=duration=30:size=1280x720:rate=30 -c:v libx264
  -g 300 -keyint_min 300 -sc_threshold 0 -preset fast longgop_h264.mp4` (10s closed GOP).
  Swap `-c:v libx265` for hevc; bump `-g 600` for 20s GOPs to stress the boundary case.
- yt-dlp a Twitch VOD or YouTube stream archive for real long-GOP h264 / vp9 material.

Stash these under a local (gitignored) `~/Videos/multitool-trim-fixtures/` so they survive
across smoke sessions.
