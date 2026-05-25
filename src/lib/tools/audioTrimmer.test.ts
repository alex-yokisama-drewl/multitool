import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { trimAudio, type Opts, type Progress } from "./audioTrimmer";

const path = "/tmp/song.wav";
const opts: Opts = {
  start_ms: 1_000,
  end_ms: 5_000,
  fade_in_ms: 1_000,
  fade_out_ms: 1_000,
};

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

const okResult = {
  output: "/tmp/song_trimmed.wav",
  warnings: [],
  duration_ms: 42,
};

describe("trimAudio", () => {
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

  it("invokes trim_audio with path and opts", async () => {
    invokeMock.mockResolvedValueOnce(okResult);

    const result = await trimAudio(path, opts);

    expect(invokeMock).toHaveBeenCalledWith(
      "trim_audio",
      expect.objectContaining({ path, opts }),
    );
    expect(result).toEqual(okResult);
  });

  it("forwards Started events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Event for a different job → ignored.
      progressHandler?.({
        payload: {
          job_id: "other-job",
          progress: { kind: "started", source: "/x.wav" },
        },
      });
      // Started event for this job.
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: { kind: "started", source: path },
        },
      });
      return Promise.resolve(okResult);
    });

    const onProgress = vi.fn();
    await trimAudio(path, opts, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(1);
    expect(onProgress).toHaveBeenCalledWith({ kind: "started", source: path });
  });

  it("unsubscribes the listener on success and on error", async () => {
    invokeMock.mockResolvedValueOnce(okResult);
    await trimAudio(path, opts);
    expect(unlistenMock).toHaveBeenCalledTimes(1);

    unlistenMock.mockReset();
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "decode failed",
    });
    await expect(trimAudio(path, opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "decode failed",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "trim_audio") {
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
      trimAudio(path, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      trimAudio(path, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
