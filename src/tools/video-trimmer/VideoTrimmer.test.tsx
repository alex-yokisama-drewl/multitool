import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  pickVideoFileMock,
  revealInFolderMock,
  videoAssetUrlMock,
  trimVideoMock,
  preparePreviewProxyMock,
  probeVideoDurationMock,
  cleanupPreviewProxyMock,
  cleanupStaleProxiesMock,
} = vi.hoisted(() => ({
  pickVideoFileMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  videoAssetUrlMock: vi.fn(),
  trimVideoMock: vi.fn(),
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
vi.mock("@/lib/tools/videoTrimmer", () => ({
  trimVideo: trimVideoMock,
  preparePreviewProxy: preparePreviewProxyMock,
  probeVideoDuration: probeVideoDurationMock,
  cleanupPreviewProxy: cleanupPreviewProxyMock,
  cleanupStaleProxies: cleanupStaleProxiesMock,
}));

import { VideoTrimmer } from "./VideoTrimmer";

const PROXY = "/tmp/multitool-preview-x.webm";

let fetchMock: ReturnType<typeof vi.fn>;
let createObjectURLMock: ReturnType<typeof vi.fn>;

function renderTool() {
  return render(
    <MemoryRouter>
      <VideoTrimmer />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  pickVideoFileMock.mockReset();
  revealInFolderMock.mockReset().mockResolvedValue(undefined);
  videoAssetUrlMock
    .mockReset()
    .mockImplementation((p: string) => `asset://${p}`);
  trimVideoMock.mockReset();
  preparePreviewProxyMock.mockReset().mockResolvedValue({ proxy_path: PROXY });
  probeVideoDurationMock.mockReset().mockResolvedValue({ duration_ms: 10_000 });
  cleanupPreviewProxyMock.mockReset().mockResolvedValue(undefined);
  cleanupStaleProxiesMock.mockReset().mockResolvedValue(undefined);

  // The preview path fetches the proxy bytes and plays them from an object
  // URL — stub both (jsdom implements neither usefully). Captured in vars
  // so assertions don't reference the globals directly (unbound-method).
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

/// Pick a file and land in the picked view (every pick transcodes a proxy
/// and plays it from a blob URL).
async function pick(path = "/tmp/clip.mp4") {
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

  it("transcodes a preview proxy and plays it from an object URL", async () => {
    renderTool();
    await pick("/tmp/movie.mkv");

    expect(probeVideoDurationMock).toHaveBeenCalledWith("/tmp/movie.mkv");
    // Every pick transcodes a proxy (the WebView can't load the source's
    // asset URL directly).
    expect(preparePreviewProxyMock).toHaveBeenCalledTimes(1);
    const [proxySource, proxyHooks] = preparePreviewProxyMock.mock.calls[0] as [
      string,
      { onProgress?: unknown },
    ];
    expect(proxySource).toBe("/tmp/movie.mkv");
    expect(typeof proxyHooks.onProgress).toBe("function");
    // Proxy bytes are fetched via the asset URL, then played from a blob.
    expect(videoAssetUrlMock).toHaveBeenCalledWith(PROXY);
    expect(fetchMock).toHaveBeenCalledWith(`asset://${PROXY}`);
    expect(createObjectURLMock).toHaveBeenCalledTimes(1);
    // Duration rendered from the probe (header line).
    expect(screen.getByText(/Duration 00:10\.000/)).toBeInTheDocument();
    // Default markers span the whole clip.
    expect(screen.getByLabelText(/^start$/i)).toHaveValue("00:00.000");
    expect(screen.getByLabelText(/^end$/i)).toHaveValue("00:10.000");
  });

  it("forwards the trim window to trimVideo and shows the result", async () => {
    trimVideoMock.mockResolvedValueOnce({
      output: "/tmp/clip_trimmed.mp4",
      duration_ms: 42,
    });
    renderTool();
    await pick();

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
    await pick();

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
    await pick();

    fireEvent.click(screen.getByRole("button", { name: /^trim$/i }));
    const cancelButton = await screen.findByRole("button", { name: /cancel/i });
    fireEvent.click(cancelButton);

    await waitFor(() => {
      expect(captured?.aborted).toBe(true);
    });
  });
});
