import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { extractAudio, type Progress } from "./audioExtractor";

const path = "/tmp/holiday.mov";

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

const okResult = {
  track_count: 2,
  outputs: ["/tmp/holiday_audio_1.mp3", "/tmp/holiday_audio_2.mp3"],
  duration_ms: 1234,
};

describe("extractAudio", () => {
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

  it("invokes extract_audio with the source path", async () => {
    invokeMock.mockResolvedValueOnce(okResult);

    const result = await extractAudio(path);

    expect(invokeMock).toHaveBeenCalledWith(
      "extract_audio",
      expect.objectContaining({ path }),
    );
    expect(result).toEqual(okResult);
  });

  it("forwards Started / FileProgress / Succeeded events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Different job — must be ignored.
      progressHandler?.({
        payload: {
          job_id: "other-job",
          progress: { kind: "started", index: 0, total: 1 },
        },
      });
      // Three events for this job covering all three variants.
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: { kind: "started", index: 0, total: 2 },
        },
      });
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: {
            kind: "file-progress",
            index: 0,
            total: 2,
            fraction: 0.42,
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
            output: "/tmp/holiday_audio_1.mp3",
          },
        },
      });
      return Promise.resolve(okResult);
    });

    const onProgress = vi.fn();
    await extractAudio(path, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(3);
    const calls = onProgress.mock.calls.map((c) => c[0] as Progress);
    const [started, fileProgress, succeeded] = calls as [
      Progress,
      Progress,
      Progress,
    ];
    expect(started.kind).toBe("started");
    expect(fileProgress.kind).toBe("file-progress");
    expect(succeeded.kind).toBe("succeeded");
    expect(fileProgress).toMatchObject({ fraction: 0.42 });
    expect(succeeded).toMatchObject({ output: "/tmp/holiday_audio_1.mp3" });
  });

  it("unsubscribes the listener on success", async () => {
    invokeMock.mockResolvedValueOnce(okResult);
    await extractAudio(path);
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the listener on error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "no audio streams",
    });
    await expect(extractAudio(path)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "no audio streams",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "extract_audio") {
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
      extractAudio(path, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("rejects with the typed envelope when invoke fails", async () => {
    const envelope = {
      kind: "FileNotFound" as const,
      message: "file not found: /tmp/missing.mov",
    };
    invokeMock.mockRejectedValueOnce(envelope);
    await expect(extractAudio(path)).rejects.toEqual(envelope);
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      extractAudio(path, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
