import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  pickVideoFileMock,
  allowMediaPreviewMock,
  revealInFolderMock,
  videoAssetUrlMock,
  trimVideoMock,
  preparePreviewProxyMock,
  probeVideoDurationMock,
  cleanupPreviewProxyMock,
  probePlayableMock,
} = vi.hoisted(() => ({
  pickVideoFileMock: vi.fn(),
  allowMediaPreviewMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  videoAssetUrlMock: vi.fn(),
  trimVideoMock: vi.fn(),
  preparePreviewProxyMock: vi.fn(),
  probeVideoDurationMock: vi.fn(),
  cleanupPreviewProxyMock: vi.fn(),
  probePlayableMock: vi.fn(),
}));

vi.mock("@/lib/system", () => ({
  pickVideoFile: pickVideoFileMock,
  allowMediaPreview: allowMediaPreviewMock,
  revealInFolder: revealInFolderMock,
  videoAssetUrl: videoAssetUrlMock,
}));
vi.mock("@/lib/tools/videoTrimmer", () => ({
  trimVideo: trimVideoMock,
  preparePreviewProxy: preparePreviewProxyMock,
  probeVideoDuration: probeVideoDurationMock,
  cleanupPreviewProxy: cleanupPreviewProxyMock,
}));
vi.mock("@/lib/videoPreview", () => ({ probePlayable: probePlayableMock }));

import { VideoTrimmer } from "./VideoTrimmer";

function renderTool() {
  return render(
    <MemoryRouter>
      <VideoTrimmer />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  pickVideoFileMock.mockReset();
  allowMediaPreviewMock.mockReset().mockResolvedValue(undefined);
  revealInFolderMock.mockReset().mockResolvedValue(undefined);
  videoAssetUrlMock
    .mockReset()
    .mockImplementation((p: string) => `asset://${p}`);
  trimVideoMock.mockReset();
  preparePreviewProxyMock.mockReset();
  probeVideoDurationMock.mockReset().mockResolvedValue({ duration_ms: 10_000 });
  cleanupPreviewProxyMock.mockReset().mockResolvedValue(undefined);
  probePlayableMock.mockReset().mockResolvedValue(true);
});

/// Pick a natively-playable file and land in the picked view.
async function pickPlayable(path = "/tmp/clip.mp4") {
  pickVideoFileMock.mockResolvedValueOnce(path);
  fireEvent.click(screen.getByRole("button", { name: /select video file/i }));
  await screen.findByRole("button", { name: /^trim$/i });
}

describe("VideoTrimmer", () => {
  it("renders the picker in the idle state", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /select video file/i }),
    ).toBeInTheDocument();
  });

  it("loads a natively-playable source without transcoding a proxy", async () => {
    renderTool();
    await pickPlayable();

    expect(probeVideoDurationMock).toHaveBeenCalledWith("/tmp/clip.mp4");
    expect(allowMediaPreviewMock).toHaveBeenCalledWith(["/tmp/clip.mp4"]);
    expect(preparePreviewProxyMock).not.toHaveBeenCalled();
    // Duration rendered from the probe.
    expect(screen.getByText(/00:10\.000/)).toBeInTheDocument();
    // Default markers span the whole clip.
    expect(screen.getByLabelText(/^start$/i)).toHaveValue("00:00.000");
    expect(screen.getByLabelText(/^end$/i)).toHaveValue("00:10.000");
  });

  it("transcodes a preview proxy when the source isn't decodable", async () => {
    probePlayableMock.mockResolvedValueOnce(false);
    preparePreviewProxyMock.mockResolvedValueOnce({
      proxy_path: "/tmp/multitool-preview-x.mp4",
    });
    renderTool();
    await pickPlayable("/tmp/movie.mkv");

    expect(preparePreviewProxyMock).toHaveBeenCalledTimes(1);
    const [proxySource, proxyHooks] = preparePreviewProxyMock.mock.calls[0] as [
      string,
      { onProgress?: unknown },
    ];
    expect(proxySource).toBe("/tmp/movie.mkv");
    expect(typeof proxyHooks.onProgress).toBe("function");
    // Player points at the proxy asset URL, not the source.
    expect(videoAssetUrlMock).toHaveBeenCalledWith(
      "/tmp/multitool-preview-x.mp4",
    );
  });

  it("forwards the trim window to trimVideo and shows the result", async () => {
    trimVideoMock.mockResolvedValueOnce({
      output: "/tmp/clip_trimmed.mp4",
      duration_ms: 42,
    });
    renderTool();
    await pickPlayable();

    fireEvent.click(screen.getByRole("button", { name: /^trim$/i }));

    await waitFor(() => {
      expect(trimVideoMock).toHaveBeenCalledTimes(1);
    });
    const [path, opts, hooks] = trimVideoMock.mock.calls[0] as [
      string,
      { start_ms: number; end_ms: number },
      { signal?: AbortSignal },
    ];
    expect(path).toBe("/tmp/clip.mp4");
    expect(opts).toEqual({ start_ms: 0, end_ms: 10_000 });
    expect(hooks.signal).toBeInstanceOf(AbortSignal);
    expect(await screen.findByText(/clip_trimmed\.mp4/)).toBeInTheDocument();
  });

  it("renders the error envelope and keeps the picked file on failure", async () => {
    trimVideoMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "invalid range",
    });
    renderTool();
    await pickPlayable();

    fireEvent.click(screen.getByRole("button", { name: /^trim$/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent("invalid range");
    // Still on the picked view (Trim button present for retry).
    expect(screen.getByRole("button", { name: /^trim$/i })).toBeInTheDocument();
  });

  it("aborts the trim signal when Cancel is clicked", async () => {
    let captured: AbortSignal | undefined;
    trimVideoMock.mockImplementation(
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
    await pickPlayable();

    fireEvent.click(screen.getByRole("button", { name: /^trim$/i }));
    const cancelButton = await screen.findByRole("button", { name: /cancel/i });
    fireEvent.click(cancelButton);

    await waitFor(() => {
      expect(captured?.aborted).toBe(true);
    });
  });
});
