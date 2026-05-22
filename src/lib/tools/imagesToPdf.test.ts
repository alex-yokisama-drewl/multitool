import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { convertImagesToPdf, type Opts, type Progress } from "./imagesToPdf";

const paths = ["/tmp/a.png", "/tmp/b.jpg"];
const opts: Opts = { page_size: "auto-fit" };

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

describe("convertImagesToPdf", () => {
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

  it("invokes convert_images_to_pdf with the paths and opts", async () => {
    invokeMock.mockResolvedValueOnce({
      output_path: "/tmp/a.pdf",
      page_count: 2,
      duration_ms: 42,
    });

    const result = await convertImagesToPdf(paths, opts);

    expect(invokeMock).toHaveBeenCalledWith(
      "convert_images_to_pdf",
      expect.objectContaining({ paths, opts }),
    );
    expect(result).toEqual({
      output_path: "/tmp/a.pdf",
      page_count: 2,
      duration_ms: 42,
    });
  });

  it("forwards progress events filtered by JobId to onProgress", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // One event for a DIFFERENT job, then two for this job — only the
      // latter two should reach onProgress.
      progressHandler?.({
        payload: { job_id: "other-job", progress: { image: 9, total: 9 } },
      });
      progressHandler?.({
        payload: { job_id: args.jobId, progress: { image: 1, total: 2 } },
      });
      progressHandler?.({
        payload: { job_id: args.jobId, progress: { image: 2, total: 2 } },
      });
      return Promise.resolve({
        output_path: "/tmp/a.pdf",
        page_count: 2,
        duration_ms: 1,
      });
    });

    const onProgress = vi.fn();
    await convertImagesToPdf(paths, opts, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(2);
    expect(onProgress).toHaveBeenNthCalledWith(1, { image: 1, total: 2 });
    expect(onProgress).toHaveBeenNthCalledWith(2, { image: 2, total: 2 });
  });

  it("unsubscribes the progress listener on success", async () => {
    invokeMock.mockResolvedValueOnce({
      output_path: "/tmp/a.pdf",
      page_count: 1,
      duration_ms: 1,
    });

    await convertImagesToPdf(paths, opts);

    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the progress listener on error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "UnsupportedFormat",
      message: "bad input",
    });

    await expect(convertImagesToPdf(paths, opts)).rejects.toEqual({
      kind: "UnsupportedFormat",
      message: "bad input",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "convert_images_to_pdf") {
        capturedJobId = args.jobId;
        controller.abort();
        // Tauri rejects with the plain `{ kind, message }` envelope — not
        // an Error instance — so we mirror that wire shape exactly.
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({
          kind: "Cancelled",
          message: "operation cancelled",
        });
      }
      return Promise.resolve();
    });

    await expect(
      convertImagesToPdf(paths, opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("rejects with the typed error envelope when invoke fails", async () => {
    const envelope = {
      kind: "FileNotFound" as const,
      message: "file not found: /tmp/missing.png",
    };
    invokeMock.mockRejectedValueOnce(envelope);

    await expect(convertImagesToPdf(paths, opts)).rejects.toEqual(envelope);
  });

  it("throws immediately without invoking when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(
      convertImagesToPdf(paths, opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
