import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  convertAudioFormat,
  type Opts,
  type Progress,
} from "./audioFormatConverter";

const paths = ["/tmp/song.wav", "/tmp/track.mp3"];
const opts: Opts = {
  target_format: "mp3",
  mp3_bitrate_kbps: 192,
  vorbis_quality: 5.0,
  flac_compression_level: 5,
  wav_bit_depth: "bit16",
  channels: "source",
};

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

const okResult = {
  success_count: 2,
  skip_count: 0,
  skipped: [],
  first_output_path: "/tmp/song.mp3",
  duration_ms: 42,
};

describe("convertAudioFormat", () => {
  let progressHandler: ProgressHandler | undefined;
  let unlistenMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    progressHandler = undefined;
    unlistenMock = vi.fn();
    listenMock.mockImplementation(
      (_event: string, handler: ProgressHandler) => {
        progressHandler = handler;
        return Promise.resolve(unlistenMock);
      },
    );
  });

  it("invokes convert_audio_format with paths and opts", async () => {
    invokeMock.mockResolvedValueOnce(okResult);

    const result = await convertAudioFormat(paths, opts);

    expect(invokeMock).toHaveBeenCalledWith(
      "convert_audio_format",
      expect.objectContaining({ paths, opts }),
    );
    expect(result).toEqual(okResult);
  });

  it("forwards Started/Succeeded/Skipped events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Event for a different job → ignored.
      progressHandler?.({
        payload: {
          job_id: "other-job",
          progress: {
            kind: "started",
            index: 0,
            total: 1,
            source: "/x.wav",
          },
        },
      });
      // Three events for this job: Started → Succeeded → Skipped.
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: {
            kind: "started",
            index: 0,
            total: 2,
            source: "/tmp/song.wav",
          },
        },
      });
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: {
            kind: "succeeded",
            index: 0,
            total: 2,
            source: "/tmp/song.wav",
            output: "/tmp/song.mp3",
            warnings: ["downmixed 6 channels to stereo"],
          },
        },
      });
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: {
            kind: "skipped",
            index: 1,
            total: 2,
            source: "/tmp/track.mp3",
            error: { kind: "UnsupportedFormat", message: "bad bytes" },
          },
        },
      });
      return Promise.resolve(okResult);
    });

    const onProgress = vi.fn();
    await convertAudioFormat(paths, opts, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(3);
    const calls = onProgress.mock.calls.map((c) => c[0] as Progress);
    const [started, succeeded, skipped] = calls as [
      Progress,
      Progress,
      Progress,
    ];
    expect(started.kind).toBe("started");
    expect(succeeded.kind).toBe("succeeded");
    expect(skipped.kind).toBe("skipped");
    expect(succeeded).toMatchObject({
      output: "/tmp/song.mp3",
      warnings: ["downmixed 6 channels to stereo"],
    });
    expect(skipped).toMatchObject({
      error: { kind: "UnsupportedFormat" },
    });
  });

  it("unsubscribes the listener on success", async () => {
    invokeMock.mockResolvedValueOnce(okResult);
    await convertAudioFormat(paths, opts);
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the listener on error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "no audio files to convert",
    });
    await expect(convertAudioFormat(paths, opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "no audio files to convert",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "convert_audio_format") {
        capturedJobId = args.jobId;
        controller.abort();
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({
          kind: "Cancelled",
          message: "operation cancelled",
        });
      }
      return Promise.resolve();
    });

    await expect(
      convertAudioFormat(paths, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("rejects with the typed envelope when invoke fails", async () => {
    const envelope = {
      kind: "FileNotFound" as const,
      message: "file not found: /tmp/missing.wav",
    };
    invokeMock.mockRejectedValueOnce(envelope);
    await expect(convertAudioFormat(paths, opts)).rejects.toEqual(envelope);
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      convertAudioFormat(paths, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
