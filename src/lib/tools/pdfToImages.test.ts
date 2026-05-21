import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { convertPdfToImages, type Opts, type Progress } from "./pdfToImages";

const opts: Opts = { format: "png", dpi: 150 };

type ProgressHandler = (event: {
  payload: { job_id: string; progress: Progress };
}) => void;

describe("convertPdfToImages", () => {
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

  it("invokes convert_pdf_to_images with the path and opts", async () => {
    invokeMock.mockResolvedValueOnce({
      output_dir: "/tmp/out",
      page_count: 3,
      duration_ms: 42,
    });

    const result = await convertPdfToImages("/tmp/in.pdf", opts);

    expect(invokeMock).toHaveBeenCalledWith(
      "convert_pdf_to_images",
      expect.objectContaining({ path: "/tmp/in.pdf", opts }),
    );
    expect(result).toEqual({
      output_dir: "/tmp/out",
      page_count: 3,
      duration_ms: 42,
    });
  });

  it("forwards progress events filtered by JobId to onProgress", async () => {
    invokeMock.mockImplementation((_cmd: string, args: { jobId: string }) => {
      // Emit one event for a DIFFERENT job, then two for this job. Only
      // the latter two should reach onProgress — proves the JobId filter.
      progressHandler?.({
        payload: { job_id: "other-job", progress: { page: 5, total: 10 } },
      });
      progressHandler?.({
        payload: { job_id: args.jobId, progress: { page: 1, total: 3 } },
      });
      progressHandler?.({
        payload: { job_id: args.jobId, progress: { page: 2, total: 3 } },
      });
      return Promise.resolve({
        output_dir: "/tmp/out",
        page_count: 3,
        duration_ms: 1,
      });
    });

    const onProgress = vi.fn();
    await convertPdfToImages("/tmp/in.pdf", opts, { onProgress });

    expect(onProgress).toHaveBeenCalledTimes(2);
    expect(onProgress).toHaveBeenNthCalledWith(1, { page: 1, total: 3 });
    expect(onProgress).toHaveBeenNthCalledWith(2, { page: 2, total: 3 });
  });

  it("unsubscribes the progress listener on success", async () => {
    invokeMock.mockResolvedValueOnce({
      output_dir: "/out",
      page_count: 1,
      duration_ms: 1,
    });

    await convertPdfToImages("/tmp/in.pdf", opts);

    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the progress listener on error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "bad pdf",
    });

    await expect(convertPdfToImages("/tmp/in.pdf", opts)).rejects.toEqual({
      kind: "ProcessingFailed",
      message: "bad pdf",
    });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === "convert_pdf_to_images") {
        capturedJobId = args.jobId;
        controller.abort();
        // The real Rust side rejects with Cancelled after the token fires.
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
      convertPdfToImages("/tmp/in.pdf", opts, { signal: controller.signal }),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("rejects with the typed error envelope when invoke fails", async () => {
    const envelope = {
      kind: "Encrypted" as const,
      message:
        "password-protected PDF; password entry is not supported in Phase 1",
    };
    invokeMock.mockRejectedValueOnce(envelope);

    await expect(convertPdfToImages("/tmp/in.pdf", opts)).rejects.toEqual(
      envelope,
    );
  });

  it("throws immediately without invoking when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(
      convertPdfToImages("/tmp/in.pdf", opts, { signal: controller.signal }),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
