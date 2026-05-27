import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  cleanupPreviewProxy,
  preparePreviewProxy,
  probeVideoDuration,
  trimVideo,
  type Opts,
  type Progress,
  type ProxyProgress,
} from "./videoTrimmer";

const path = "/tmp/holiday.mkv";
const opts: Opts = { start_ms: 1_000, end_ms: 5_000 };

const trimResult = { output: "/tmp/holiday_trimmed.mkv", duration_ms: 87 };

type Handler<P> = (event: { payload: { job_id: string; progress: P } }) => void;

describe("trimVideo", () => {
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

  it("invokes trim_video with path and opts", async () => {
    invokeMock.mockResolvedValueOnce(trimResult);
    const result = await trimVideo(path, opts);
    expect(invokeMock).toHaveBeenCalledWith(
      "trim_video",
      expect.objectContaining({ path, opts }),
    );
    expect(result).toEqual(trimResult);
  });

  it("forwards started / file-progress events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Different job → ignored.
      progressHandler?.({
        payload: {
          job_id: "other",
          progress: { kind: "started", source: "/x.mkv" },
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
          progress: { kind: "file-progress", source: path, fraction: 0.5 },
        },
      });
      return Promise.resolve(trimResult);
    });

    const onProgress = vi.fn();
    await trimVideo(path, opts, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(2);
    const calls = onProgress.mock.calls.map((c) => c[0] as Progress);
    const [started, fileProgress] = calls as [Progress, Progress];
    expect(started.kind).toBe("started");
    expect(fileProgress).toMatchObject({
      kind: "file-progress",
      fraction: 0.5,
    });
  });

  it("unsubscribes the listener on success and on error", async () => {
    invokeMock.mockResolvedValueOnce(trimResult);
    await trimVideo(path, opts);
    expect(unlistenMock).toHaveBeenCalledTimes(1);

    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "invalid range",
    });
    await expect(trimVideo(path, opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "invalid range",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(2);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;
    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "trim_video") {
        capturedJobId = args.jobId;
        controller.abort();
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({ kind: "Cancelled", message: "cancelled" });
      }
      return Promise.resolve();
    });

    await expect(
      trimVideo(path, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "cancelled" });
    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      trimVideo(path, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});

describe("preparePreviewProxy", () => {
  let progressHandler: Handler<ProxyProgress> | undefined;

  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    progressHandler = undefined;
    listenMock.mockImplementation(
      (_event: string, handler: Handler<ProxyProgress>) => {
        progressHandler = handler;
        return Promise.resolve(vi.fn());
      },
    );
  });

  it("invokes prepare_preview_proxy and forwards transcode progress", async () => {
    const proxyResult = { proxy_path: "/tmp/multitool-preview-abc.mp4" };
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      progressHandler?.({
        payload: { job_id: args.jobId, progress: { fraction: 0.3 } },
      });
      return Promise.resolve(proxyResult);
    });

    const onProgress = vi.fn();
    const result = await preparePreviewProxy(path, { onProgress });

    expect(invokeMock).toHaveBeenCalledWith(
      "prepare_preview_proxy",
      expect.objectContaining({ path }),
    );
    expect(onProgress).toHaveBeenCalledWith({ fraction: 0.3 });
    expect(result).toEqual(proxyResult);
  });
});

describe("probeVideoDuration / cleanupPreviewProxy", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("probeVideoDuration invokes the command and returns the duration", async () => {
    invokeMock.mockResolvedValueOnce({ duration_ms: 12_345 });
    const result = await probeVideoDuration(path);
    expect(invokeMock).toHaveBeenCalledWith("probe_video_duration", { path });
    expect(result).toEqual({ duration_ms: 12_345 });
  });

  it("probeVideoDuration rejects with the typed envelope", async () => {
    const envelope = { kind: "FileNotFound", message: "no such file" };
    invokeMock.mockRejectedValueOnce(envelope);
    await expect(probeVideoDuration(path)).rejects.toEqual(envelope);
  });

  it("cleanupPreviewProxy invokes the command with the proxy path", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await cleanupPreviewProxy("/tmp/multitool-preview-abc.mp4");
    expect(invokeMock).toHaveBeenCalledWith("cleanup_preview_proxy", {
      path: "/tmp/multitool-preview-abc.mp4",
    });
  });
});
