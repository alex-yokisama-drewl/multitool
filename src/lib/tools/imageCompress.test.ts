import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  compressImages,
  type JobResult,
  type Opts,
  type Progress,
} from "./imageCompress";

const paths = ["/tmp/red.png", "/tmp/blue.jpg"];
const opts: Opts = { quality: 0.8 };
const compressResult: JobResult = {
  success_count: 2,
  skip_count: 0,
  outputs: [
    {
      source: paths[0]!,
      output: "/tmp/red_compressed.webp",
      original_bytes: 1024,
      compressed_bytes: 512,
    },
    {
      source: paths[1]!,
      output: "/tmp/blue_compressed.jpg",
      original_bytes: 4096,
      compressed_bytes: 2048,
    },
  ],
  skipped: [],
  first_output_path: "/tmp/red_compressed.webp",
  duration_ms: 5_000,
};

type Handler<P> = (event: { payload: { job_id: string; progress: P } }) => void;

describe("compressImages", () => {
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

  it("invokes compress_images with paths and opts", async () => {
    invokeMock.mockResolvedValueOnce(compressResult);
    const result = await compressImages(paths, opts);
    expect(invokeMock).toHaveBeenCalledWith(
      "compress_images",
      expect.objectContaining({ paths, opts }),
    );
    expect(result).toEqual(compressResult);
  });

  it("forwards per-file events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Different job → ignored.
      progressHandler?.({
        payload: {
          job_id: "other",
          progress: {
            kind: "started",
            index: 0,
            total: 1,
            source: "/x.png",
          },
        },
      });
      // Same job → forwarded.
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: {
            kind: "started",
            index: 0,
            total: 2,
            source: paths[0]!,
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
            source: paths[0]!,
            output: "/tmp/red_compressed.webp",
            original_bytes: 1024,
            compressed_bytes: 512,
          },
        },
      });
      return Promise.resolve(compressResult);
    });

    const onProgress = vi.fn();
    await compressImages(paths, opts, { onProgress });
    expect(onProgress).toHaveBeenCalledTimes(2);
    const calls = onProgress.mock.calls.map((c) => c[0] as Progress);
    expect(calls[0]?.kind).toBe("started");
    expect(calls[1]).toMatchObject({
      kind: "succeeded",
      compressed_bytes: 512,
      original_bytes: 1024,
    });
  });

  it("unsubscribes the listener on success and on error", async () => {
    invokeMock.mockResolvedValueOnce(compressResult);
    await compressImages(paths, opts);
    expect(unlistenMock).toHaveBeenCalledTimes(1);

    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "encode failed",
    });
    await expect(compressImages(paths, opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "encode failed",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(2);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;
    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "compress_images") {
        capturedJobId = args.jobId;
        controller.abort();
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({ kind: "Cancelled", message: "cancelled" });
      }
      return Promise.resolve();
    });

    await expect(
      compressImages(paths, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "cancelled" });
    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      compressImages(paths, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
