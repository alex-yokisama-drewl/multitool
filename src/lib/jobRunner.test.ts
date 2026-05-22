import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { runJob } from "./jobRunner";

interface FakeProgress {
  page: number;
  total: number;
}

interface FakeResult {
  output_dir: string;
}

type ProgressHandler = (event: {
  payload: { job_id: string; progress: FakeProgress };
}) => void;

const CMD = "fake_command";

describe("runJob", () => {
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

  it("invokes the command with the supplied args and a fresh jobId", async () => {
    invokeMock.mockResolvedValueOnce({ output_dir: "/tmp/out" });

    const result = await runJob<{ path: string }, FakeProgress, FakeResult>(
      CMD,
      { path: "/in" },
    );

    expect(invokeMock).toHaveBeenCalledTimes(1);
    const [cmd, args] = invokeMock.mock.calls[0] as [
      string,
      { jobId: string; path: string },
    ];
    expect(cmd).toBe(CMD);
    expect(args.path).toBe("/in");
    expect(typeof args.jobId).toBe("string");
    expect(args.jobId.length).toBeGreaterThan(0);
    expect(result).toEqual({ output_dir: "/tmp/out" });
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
      return Promise.resolve({ output_dir: "/tmp/out" });
    });

    const onProgress = vi.fn();
    await runJob<{ path: string }, FakeProgress, FakeResult>(
      CMD,
      { path: "/in" },
      { onProgress },
    );

    expect(onProgress).toHaveBeenCalledTimes(2);
    expect(onProgress).toHaveBeenNthCalledWith(1, { page: 1, total: 3 });
    expect(onProgress).toHaveBeenNthCalledWith(2, { page: 2, total: 3 });
  });

  it("unsubscribes the progress listener on success", async () => {
    invokeMock.mockResolvedValueOnce({ output_dir: "/out" });

    await runJob<{ path: string }, FakeProgress, FakeResult>(CMD, {
      path: "/in",
    });

    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unsubscribes the progress listener on error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "boom",
    });

    await expect(
      runJob<{ path: string }, FakeProgress, FakeResult>(CMD, { path: "/in" }),
    ).rejects.toEqual({ kind: "ProcessingFailed", message: "boom" });
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("calls cancel_job with the JobId when the AbortSignal aborts", async () => {
    const controller = new AbortController();
    let capturedJobId: string | undefined;

    invokeMock.mockImplementation((cmd: string, args: { jobId?: string }) => {
      if (cmd === CMD) {
        capturedJobId = args.jobId;
        controller.abort();
        // Rust rejects with the plain `{ kind, message }` envelope — not
        // an Error instance — so mirror that wire shape exactly.
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
        return Promise.reject({
          kind: "Cancelled",
          message: "operation cancelled",
        });
      }
      return Promise.resolve();
    });

    await expect(
      runJob<{ path: string }, FakeProgress, FakeResult>(
        CMD,
        { path: "/in" },
        { signal: controller.signal },
      ),
    ).rejects.toEqual({ kind: "Cancelled", message: "operation cancelled" });

    expect(invokeMock).toHaveBeenCalledWith("cancel_job", {
      jobId: capturedJobId,
    });
  });

  it("throws immediately without invoking when the signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(
      runJob<{ path: string }, FakeProgress, FakeResult>(
        CMD,
        { path: "/in" },
        { signal: controller.signal },
      ),
    ).rejects.toBeDefined();
    expect(invokeMock).not.toHaveBeenCalled();
    expect(listenMock).not.toHaveBeenCalled();
  });
});
