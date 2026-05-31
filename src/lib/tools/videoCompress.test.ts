import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { compressVideo, type Opts, type Progress } from "./videoCompress";

const path = "/tmp/holiday.mp4";
const opts: Opts = { quality: 0.5 };
const compressResult = {
  output: "/tmp/holiday_compressed.mp4",
  duration_ms: 12_345,
};

type Handler<P> = (event: { payload: { job_id: string; progress: P } }) => void;

describe("compressVideo", () => {
  let progressHandler: Handler<Progress> | undefined;
  let unlistenMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    progressHandler = undefined;
    unlistenMock = vi.fn();
    listenMock.mockImplementation(
      (_event: string, handler: Handler<Progress>) => {
        progressHandler = handler;
        return Promise.resolve(unlistenMock);
      },
    );
  });

  it("invokes compress_video with path and opts", async () => {
    invokeMock.mockResolvedValueOnce(compressResult);
    const result = await compressVideo(path, opts);
    expect(invokeMock).toHaveBeenCalledWith(
      "compress_video",
      expect.objectContaining({ path, opts }),
    );
    expect(result).toEqual(compressResult);
  });

  it("forwards started / file-progress events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Different job → ignored.
      progressHandler?.({
        payload: {
          job_id: "other",
          progress: { kind: "started", source: "/x.mp4" },
        },
      });
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: { kind: "started", source: path },
        },
      });
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: { kind: "file-progress", source: path, fraction: 0.4 },
        },
      });
      return Promise.resolve(compressResult);
    });

    const onProgress = vi.fn();
    await compressVideo(path, opts, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(2);
    const calls = onProgress.mock.calls.map((c) => c[0] as Progress);
    const [started, fileProgress] = calls as [Progress, Progress];
    expect(started.kind).toBe("started");
    expect(fileProgress).toMatchObject({
      kind: "file-progress",
      fraction: 0.4,
    });
  });

  it("unsubscribes the listener on success and on error", async () => {
    invokeMock.mockResolvedValueOnce(compressResult);
    await compressVideo(path, opts);
    expect(unlistenMock).toHaveBeenCalledTimes(1);

    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "encode failed",
    });
    await expect(compressVideo(path, opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "encode failed",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(2);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;
    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "compress_video") {
        capturedJobId = args.jobId;
        controller.abort();
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({ kind: "Cancelled", message: "cancelled" });
      }
      return Promise.resolve();
    });

    await expect(
      compressVideo(path, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "cancelled" });
    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      compressVideo(path, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
