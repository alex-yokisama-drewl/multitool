import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  pickConvertibleImagesMock,
  revealInFolderMock,
  fileSizeMock,
  allowMediaPreviewMock,
  imageAssetUrlMock,
  compressImagesMock,
} = vi.hoisted(() => ({
  pickConvertibleImagesMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  fileSizeMock: vi.fn(),
  allowMediaPreviewMock: vi.fn(),
  imageAssetUrlMock: vi.fn((p: string) => `asset://${p}`),
  compressImagesMock: vi.fn(),
}));

vi.mock("@/lib/system", () => ({
  pickConvertibleImages: pickConvertibleImagesMock,
  revealInFolder: revealInFolderMock,
  fileSize: fileSizeMock,
  allowMediaPreview: allowMediaPreviewMock,
  imageAssetUrl: imageAssetUrlMock,
}));
vi.mock("@/lib/tools/imageCompress", () => ({
  compressImages: compressImagesMock,
}));

import { ImageCompress } from "./ImageCompress";

function renderTool() {
  return render(
    <MemoryRouter>
      <ImageCompress />
    </MemoryRouter>,
  );
}

const PNG = "/tmp/red.png";
const JPG = "/tmp/blue.jpg";

async function pick(paths = [PNG, JPG]) {
  pickConvertibleImagesMock.mockResolvedValueOnce(paths);
  fireEvent.click(screen.getByRole("button", { name: /select image files/i }));
  await screen.findByRole("button", { name: /^compress$/i });
}

beforeEach(() => {
  pickConvertibleImagesMock.mockReset();
  revealInFolderMock.mockReset().mockResolvedValue(undefined);
  fileSizeMock.mockReset().mockResolvedValue(1024);
  allowMediaPreviewMock.mockReset().mockResolvedValue(undefined);
  imageAssetUrlMock.mockClear();
  compressImagesMock.mockReset();
});

describe("ImageCompress", () => {
  it("renders the picker in the idle state", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /select image files/i }),
    ).toBeInTheDocument();
  });

  it("shows staged rows with predicted output extensions and a default-0.8 slider", async () => {
    renderTool();
    await pick();
    // Both source filenames + their predicted output names render.
    expect(screen.getByText("red.png")).toBeInTheDocument();
    expect(screen.getByText("blue.jpg")).toBeInTheDocument();
    // PNG → .webp (strategy), JPEG keeps .jpg spelling.
    expect(screen.getByText("→ red_compressed.webp")).toBeInTheDocument();
    expect(screen.getByText("→ blue_compressed.jpg")).toBeInTheDocument();
    // Slider defaults to ~0.8 (800/1000).
    const slider = screen.getByRole("slider", { name: /^quality$/i });
    expect(slider).toHaveValue("800");
  });

  it("forwards the picked paths and slider value to compressImages", async () => {
    compressImagesMock.mockResolvedValueOnce({
      success_count: 2,
      skip_count: 0,
      outputs: [
        {
          source: PNG,
          output: "/tmp/red_compressed.webp",
          original_bytes: 1024,
          compressed_bytes: 512,
        },
        {
          source: JPG,
          output: "/tmp/blue_compressed.jpg",
          original_bytes: 4096,
          compressed_bytes: 2048,
        },
      ],
      skipped: [],
      first_output_path: "/tmp/red_compressed.webp",
      duration_ms: 100,
    });
    renderTool();
    await pick();

    // Move the slider toward "smaller file" (~0.3 quality).
    const slider = screen.getByRole("slider", { name: /^quality$/i });
    fireEvent.change(slider, { target: { value: "300" } });
    fireEvent.click(screen.getByRole("button", { name: /^compress$/i }));

    await waitFor(() => {
      expect(compressImagesMock).toHaveBeenCalledTimes(1);
    });
    const [paths, opts, hooks] = compressImagesMock.mock.calls[0] as [
      string[],
      { quality: number },
      { signal?: AbortSignal },
    ];
    expect(paths).toEqual([PNG, JPG]);
    expect(opts.quality).toBeCloseTo(0.3, 3);
    expect(hooks.signal).toBeInstanceOf(AbortSignal);

    // Done view renders the per-file size delta. Both outputs happen to
    // hit -50% in this fixture (1024→512 and 4096→2048).
    expect(await screen.findByText(/red_compressed\.webp/)).toBeInTheDocument();
    expect(screen.getAllByText(/-50%/)).toHaveLength(2);
  });

  it("renders the error envelope and keeps the staged files on failure", async () => {
    compressImagesMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "encode failed",
    });
    renderTool();
    await pick();

    fireEvent.click(screen.getByRole("button", { name: /^compress$/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent("encode failed");
    // Still on the staging view — Compress button + the filenames are
    // present so the user can adjust the slider and retry without
    // re-picking.
    expect(
      screen.getByRole("button", { name: /^compress$/i }),
    ).toBeInTheDocument();
    expect(screen.getByText("red.png")).toBeInTheDocument();
  });

  it("renders the WebP-redirect banner when PNG/BMP/TIFF inputs are present", async () => {
    renderTool();
    // Mixed batch: PNG + BMP + TIFF will redirect to WebP; JPEG stays JPEG.
    await pick(["/tmp/a.png", "/tmp/b.bmp", "/tmp/c.tif", "/tmp/d.jpg"]);

    const banner = screen.getByRole("status");
    // Plural-aware total (3 PNG/BMP/TIFF, not 4 — JPEG isn't redirected).
    expect(banner).toHaveTextContent(/3 files will be saved as WebP/i);
    // Per-format breakdown in stable order.
    expect(banner).toHaveTextContent(/PNG \(1\), BMP \(1\), TIF \(1\)/i);
  });

  it("omits the WebP-redirect banner when the batch is JPEG-only", async () => {
    renderTool();
    await pick(["/tmp/a.jpg", "/tmp/b.jpeg"]);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("aborts the compress signal when Cancel is clicked", async () => {
    let captured: AbortSignal | undefined;
    compressImagesMock.mockImplementation(
      (_paths: string[], _opts: unknown, hooks: { signal?: AbortSignal }) => {
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
