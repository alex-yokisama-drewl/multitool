import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  convertImageFormat,
  type Opts,
  type Progress,
} from "./imageFormatConverter";

const paths = ["/tmp/a.png", "/tmp/b.jpg"];
const opts: Opts = {
  target_format: "jpeg",
  jpeg_quality: 85,
  alpha_handling: "flatten-white",
  svg_raster_size: { "longest-edge-px": 1024 },
};

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

const okResult = {
  success_count: 2,
  skip_count: 0,
  skipped: [],
  first_output_path: "/tmp/a.jpg",
  duration_ms: 13,
};

describe("convertImageFormat", () => {
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

  it("invokes convert_image_format with paths and opts", async () => {
    invokeMock.mockResolvedValueOnce(okResult);

    const result = await convertImageFormat(paths, opts);

    expect(invokeMock).toHaveBeenCalledWith(
      "convert_image_format",
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
            source: "/x.png",
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
            source: "/tmp/a.png",
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
            source: "/tmp/a.png",
            output: "/tmp/a.jpg",
            warnings: [],
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
            source: "/tmp/b.jpg",
            error: { kind: "UnsupportedFormat", message: "bad bytes" },
          },
        },
      });
      return Promise.resolve(okResult);
    });

    const onProgress = vi.fn();
    await convertImageFormat(paths, opts, { onProgress });

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
    expect(succeeded).toMatchObject({ output: "/tmp/a.jpg" });
    expect(skipped).toMatchObject({
      error: { kind: "UnsupportedFormat" },
    });
  });

  it("unsubscribes the listener on success", async () => {
    invokeMock.mockResolvedValueOnce(okResult);
    await convertImageFormat(paths, opts);
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the listener on error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "no images to convert",
    });
    await expect(convertImageFormat(paths, opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "no images to convert",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "convert_image_format") {
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
      convertImageFormat(paths, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("rejects with the typed envelope when invoke fails", async () => {
    const envelope = {
      kind: "FileNotFound" as const,
      message: "file not found: /tmp/missing.png",
    };
    invokeMock.mockRejectedValueOnce(envelope);
    await expect(convertImageFormat(paths, opts)).rejects.toEqual(envelope);
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      convertImageFormat(paths, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
