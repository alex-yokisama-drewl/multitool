import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { cropImage, type CropRect, type Progress } from "./imageCrop";

const path = "/tmp/photo.png";
const rect: CropRect = { x: 10, y: 5, width: 40, height: 20 };

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

const okResult = { output: "/tmp/photo_cropped.png", duration_ms: 12 };

describe("cropImage", () => {
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

  it("invokes crop_image with path and the rect as opts", async () => {
    invokeMock.mockResolvedValueOnce(okResult);

    const result = await cropImage(path, rect);

    expect(invokeMock).toHaveBeenCalledWith(
      "crop_image",
      expect.objectContaining({ path, opts: rect }),
    );
    expect(result).toEqual(okResult);
  });

  it("forwards Started events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Event for a different job → ignored.
      progressHandler?.({
        payload: {
          job_id: "other-job",
          progress: { kind: "started", source: "/x.png" },
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
    await cropImage(path, rect, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(1);
    expect(onProgress).toHaveBeenCalledWith({ kind: "started", source: path });
  });

  it("unsubscribes the listener on success and on error", async () => {
    invokeMock.mockResolvedValueOnce(okResult);
    await cropImage(path, rect);
    expect(unlistenMock).toHaveBeenCalledTimes(1);

    unlistenMock.mockReset();
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "crop rectangle does not intersect the image",
    });
    await expect(cropImage(path, rect)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "crop rectangle does not intersect the image",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "crop_image") {
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
      cropImage(path, rect, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      cropImage(path, rect, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
