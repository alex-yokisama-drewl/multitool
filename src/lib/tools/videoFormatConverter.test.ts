import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import {
  convertVideoFormat,
  type Opts,
  type Progress,
} from "./videoFormatConverter";

const paths = ["/tmp/holiday.mov", "/tmp/clip.mp4"];
const opts: Opts = { target_format: "mp4" };

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

const okResult = {
  success_count: 2,
  skip_count: 0,
  skipped: [],
  first_output_path: "/tmp/holiday_converted.mp4",
  duration_ms: 1234,
};

describe("convertVideoFormat", () => {
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

  it("invokes convert_video_format with paths and opts", async () => {
    invokeMock.mockResolvedValueOnce(okResult);

    const result = await convertVideoFormat(paths, opts);

    expect(invokeMock).toHaveBeenCalledWith(
      "convert_video_format",
      expect.objectContaining({ paths, opts }),
    );
    expect(result).toEqual(okResult);
  });

  it("forwards Started / FileProgress / Succeeded / Skipped events filtered by JobId", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Event for a different job → ignored.
      progressHandler?.({
        payload: {
          job_id: "other-job",
          progress: {
            kind: "started",
            index: 0,
            total: 1,
            source: "/x.mp4",
          },
        },
      });
      // Four events for this job: Started → FileProgress → Succeeded → Skipped.
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: {
            kind: "started",
            index: 0,
            total: 2,
            source: "/tmp/holiday.mov",
          },
        },
      });
      progressHandler?.({
        payload: {
          job_id: args.jobId,
          progress: {
            kind: "file-progress",
            index: 0,
            total: 2,
            source: "/tmp/holiday.mov",
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
            source: "/tmp/holiday.mov",
            output: "/tmp/holiday_converted.mp4",
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
            source: "/tmp/clip.mp4",
            error: {
              kind: "ProcessingFailed",
              message: "ffmpeg exited with status 1: …",
            },
          },
        },
      });
      return Promise.resolve(okResult);
    });

    const onProgress = vi.fn();
    await convertVideoFormat(paths, opts, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(4);
    const calls = onProgress.mock.calls.map((c) => c[0] as Progress);
    const [started, fileProgress, succeeded, skipped] = calls as [
      Progress,
      Progress,
      Progress,
      Progress,
    ];
    expect(started.kind).toBe("started");
    expect(fileProgress.kind).toBe("file-progress");
    expect(succeeded.kind).toBe("succeeded");
    expect(skipped.kind).toBe("skipped");
    expect(fileProgress).toMatchObject({ fraction: 0.42 });
    expect(succeeded).toMatchObject({ output: "/tmp/holiday_converted.mp4" });
    expect(skipped).toMatchObject({
      error: { kind: "ProcessingFailed" },
    });
  });

  it("unsubscribes the listener on success", async () => {
    invokeMock.mockResolvedValueOnce(okResult);
    await convertVideoFormat(paths, opts);
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the listener on error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "no video files to convert",
    });
    await expect(convertVideoFormat(paths, opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "no video files to convert",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "convert_video_format") {
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
      convertVideoFormat(paths, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("rejects with the typed envelope when invoke fails", async () => {
    const envelope = {
      kind: "FileNotFound" as const,
      message: "file not found: /tmp/missing.mp4",
    };
    invokeMock.mockRejectedValueOnce(envelope);
    await expect(convertVideoFormat(paths, opts)).rejects.toEqual(envelope);
  });

  it("throws immediately when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      convertVideoFormat(paths, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
