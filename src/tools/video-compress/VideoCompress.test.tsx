import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  pickVideoFileMock,
  revealInFolderMock,
  videoAssetUrlMock,
  compressVideoMock,
  preparePreviewProxyMock,
  probeVideoDurationMock,
  cleanupPreviewProxyMock,
  cleanupStaleProxiesMock,
} = vi.hoisted(() => ({
  pickVideoFileMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  videoAssetUrlMock: vi.fn(),
  compressVideoMock: vi.fn(),
  preparePreviewProxyMock: vi.fn(),
  probeVideoDurationMock: vi.fn(),
  cleanupPreviewProxyMock: vi.fn(),
  cleanupStaleProxiesMock: vi.fn(),
}));

vi.mock("@/lib/system", () => ({
  pickVideoFile: pickVideoFileMock,
  revealInFolder: revealInFolderMock,
  videoAssetUrl: videoAssetUrlMock,
}));
// Preview-proxy + duration-probe wrappers live in videoTrimmer.ts.
vi.mock("@/lib/tools/videoTrimmer", () => ({
  preparePreviewProxy: preparePreviewProxyMock,
  probeVideoDuration: probeVideoDurationMock,
  cleanupPreviewProxy: cleanupPreviewProxyMock,
  cleanupStaleProxies: cleanupStaleProxiesMock,
}));
vi.mock("@/lib/tools/videoCompress", () => ({
  compressVideo: compressVideoMock,
}));

import { VideoCompress } from "./VideoCompress";

const PROXY = "/tmp/multitool-preview-x.webm";

let fetchMock: ReturnType<typeof vi.fn>;
let createObjectURLMock: ReturnType<typeof vi.fn>;

function renderTool() {
  return render(
    <MemoryRouter>
      <VideoCompress />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  pickVideoFileMock.mockReset();
  revealInFolderMock.mockReset().mockResolvedValue(undefined);
  videoAssetUrlMock
    .mockReset()
    .mockImplementation((p: string) => `asset://${p}`);
  compressVideoMock.mockReset();
  preparePreviewProxyMock.mockReset().mockResolvedValue({ proxy_path: PROXY });
  probeVideoDurationMock.mockReset().mockResolvedValue({ duration_ms: 10_000 });
  cleanupPreviewProxyMock.mockReset().mockResolvedValue(undefined);
  cleanupStaleProxiesMock.mockReset().mockResolvedValue(undefined);

  fetchMock = vi.fn().mockResolvedValue({
    ok: true,
    status: 200,
    arrayBuffer: () => Promise.resolve(new ArrayBuffer(8)),
  });
  createObjectURLMock = vi.fn(() => "blob:preview");
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("URL", {
    ...URL,
    createObjectURL: createObjectURLMock,
    revokeObjectURL: vi.fn(),
  });
});

/// Pick a file and land in the picked view.
async function pick(path = "/tmp/clip.mp4") {
  pickVideoFileMock.mockResolvedValueOnce(path);
  fireEvent.click(screen.getByRole("button", { name: /select video file/i }));
  await screen.findByRole("button", { name: /^compress$/i });
}

describe("VideoCompress", () => {
  it("renders the picker in the idle state", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /select video file/i }),
    ).toBeInTheDocument();
  });

  it("renders the quality slider after a pick", async () => {
    renderTool();
    await pick();

    expect(probeVideoDurationMock).toHaveBeenCalledWith("/tmp/clip.mp4");
    expect(preparePreviewProxyMock).toHaveBeenCalledTimes(1);
    // Slider defaults to the midpoint (500/1000).
    const slider = screen.getByRole("slider", { name: /^quality$/i });
    expect(slider).toHaveValue("500");
  });

  it("forwards the slider value to compressVideo", async () => {
    compressVideoMock.mockResolvedValueOnce({
      output: "/tmp/clip_compressed.mp4",
      duration_ms: 42,
    });
    renderTool();
    await pick();

    // Move the slider toward "smaller file" (~0.2 quality).
    const slider = screen.getByRole("slider", { name: /^quality$/i });
    fireEvent.change(slider, { target: { value: "200" } });
    fireEvent.click(screen.getByRole("button", { name: /^compress$/i }));

    await waitFor(() => {
      expect(compressVideoMock).toHaveBeenCalledTimes(1);
    });
    const [path, opts, hooks] = compressVideoMock.mock.calls[0] as [
      string,
      { quality: number },
      { signal?: AbortSignal },
    ];
    expect(path).toBe("/tmp/clip.mp4");
    expect(opts.quality).toBeCloseTo(0.2, 3);
    expect(hooks.signal).toBeInstanceOf(AbortSignal);
    expect(await screen.findByText(/clip_compressed\.mp4/)).toBeInTheDocument();
  });

  it("renders the error envelope and keeps the picked file on failure", async () => {
    compressVideoMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "encode failed",
    });
    renderTool();
    await pick();

    fireEvent.click(screen.getByRole("button", { name: /^compress$/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent("encode failed");
    // Still on the picked view — Compress button present for retry.
    expect(
      screen.getByRole("button", { name: /^compress$/i }),
    ).toBeInTheDocument();
  });

  it("aborts the compress signal when Cancel is clicked", async () => {
    let captured: AbortSignal | undefined;
    compressVideoMock.mockImplementation(
      (_path: string, _opts: unknown, hooks: { signal?: AbortSignal }) => {
        captured = hooks.signal;
        return new Promise((_resolve, reject) => {
          hooks.signal?.addEventListener("abort", () => {
            // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
            reject({ kind: "Cancelled", message: "cancelled" });
          });
        });
      },
    );
    renderTool();
    await pick();

    fireEvent.click(screen.getByRole("button", { name: /^compress$/i }));
    const cancelButton = await screen.findByRole("button", { name: /cancel/i });
    fireEvent.click(cancelButton);

    await waitFor(() => {
      expect(captured?.aborted).toBe(true);
    });
  });
});
