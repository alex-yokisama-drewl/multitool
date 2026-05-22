import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  convertMock,
  pickConvertibleImagesMock,
  revealInFolderMock,
  allowImagePreviewMock,
  imageAssetUrlMock,
} = vi.hoisted(() => ({
  convertMock: vi.fn(),
  pickConvertibleImagesMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  allowImagePreviewMock: vi.fn(),
  imageAssetUrlMock: vi.fn((path: string) => `asset://${path}`),
}));

vi.mock("@/lib/tools/imageFormatConverter", () => ({
  convertImageFormat: convertMock,
}));
vi.mock("@/lib/system", () => ({
  pickConvertibleImages: pickConvertibleImagesMock,
  revealInFolder: revealInFolderMock,
  allowImagePreview: allowImagePreviewMock,
  imageAssetUrl: imageAssetUrlMock,
}));

import { ImageFormatConverter } from "./ImageFormatConverter";

function renderTool() {
  return render(
    <MemoryRouter>
      <ImageFormatConverter />
    </MemoryRouter>,
  );
}

async function pickInto(paths: string[]) {
  pickConvertibleImagesMock.mockResolvedValueOnce(paths);
  allowImagePreviewMock.mockResolvedValueOnce(undefined);
  fireEvent.click(screen.getByRole("button", { name: /^select images$/i }));
  await screen.findByRole("button", { name: /^convert$/i });
}

describe("ImageFormatConverter", () => {
  beforeEach(() => {
    convertMock.mockReset();
    pickConvertibleImagesMock.mockReset();
    revealInFolderMock.mockReset();
    allowImagePreviewMock.mockReset();
  });

  it("renders the idle state with a Select images button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /^select images$/i }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("Select different images REPLACES the staged batch (no merge)", async () => {
    renderTool();
    await pickInto(["/tmp/a.png", "/tmp/b.jpg"]);
    expect(screen.getByText(/staged \(2\)/i)).toBeInTheDocument();

    // Pick a single new file via "Select different images" — batch
    // becomes just that file. The previous two are discarded.
    pickConvertibleImagesMock.mockResolvedValueOnce(["/tmp/c.png"]);
    allowImagePreviewMock.mockResolvedValueOnce(undefined);
    fireEvent.click(
      screen.getByRole("button", { name: /select different images/i }),
    );
    await waitFor(() => {
      expect(screen.getByText(/staged \(1\)/i)).toBeInTheDocument();
    });
    // a.png and b.jpg are gone.
    expect(screen.queryByText("a.png")).not.toBeInTheDocument();
    expect(screen.queryByText("b.jpg")).not.toBeInTheDocument();
  });

  it("Select different images preserves the batch if the picker is cancelled", async () => {
    renderTool();
    await pickInto(["/tmp/a.png", "/tmp/b.jpg"]);
    pickConvertibleImagesMock.mockResolvedValueOnce(null);
    fireEvent.click(
      screen.getByRole("button", { name: /select different images/i }),
    );
    await waitFor(() => {
      expect(pickConvertibleImagesMock).toHaveBeenCalledTimes(2);
    });
    expect(screen.getByText(/staged \(2\)/i)).toBeInTheDocument();
  });

  it("stays in idle when the picker is cancelled (null)", async () => {
    renderTool();
    pickConvertibleImagesMock.mockResolvedValueOnce(null);
    fireEvent.click(
      screen.getByRole("button", { name: /^select images$/i }),
    );
    await waitFor(() => {
      expect(pickConvertibleImagesMock).toHaveBeenCalledTimes(1);
    });
    expect(
      screen.queryByRole("button", { name: /^convert$/i }),
    ).not.toBeInTheDocument();
  });

  it("shows JPEG quality only when target = JPEG", async () => {
    renderTool();
    await pickInto(["/tmp/a.png"]);

    // Default is PNG → no quality input.
    expect(screen.queryByLabelText(/jpeg quality/i)).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("JPEG"));
    expect(await screen.findByLabelText(/jpeg quality/i)).toBeInTheDocument();
  });

  it("shows alpha handling only when the target lacks alpha (JPEG/BMP)", async () => {
    renderTool();
    await pickInto(["/tmp/a.png"]);
    // PNG default → no alpha handling.
    expect(screen.queryByText(/alpha handling/i)).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("JPEG"));
    expect(await screen.findByText(/alpha handling/i)).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("WebP (lossless)"));
    await waitFor(() => {
      expect(screen.queryByText(/alpha handling/i)).not.toBeInTheDocument();
    });
  });

  it("shows the SVG raster-size controls only when at least one staged input is .svg", async () => {
    renderTool();
    await pickInto(["/tmp/a.png"]);
    expect(screen.queryByText(/svg raster size/i)).not.toBeInTheDocument();

    // Replace the batch with a set that includes an SVG.
    pickConvertibleImagesMock.mockResolvedValueOnce([
      "/tmp/a.png",
      "/tmp/icon.svg",
    ]);
    allowImagePreviewMock.mockResolvedValueOnce(undefined);
    fireEvent.click(
      screen.getByRole("button", { name: /select different images/i }),
    );
    expect(await screen.findByText(/svg raster size/i)).toBeInTheDocument();
  });

  it("forwards paths + Opts payload to convertImageFormat", async () => {
    convertMock.mockResolvedValueOnce({
      success_count: 1,
      skip_count: 0,
      skipped: [],
      first_output_path: "/tmp/a.png",
      duration_ms: 7,
    });

    renderTool();
    await pickInto(["/tmp/a.png"]);
    fireEvent.click(screen.getByLabelText("JPEG"));
    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await screen.findByText(/done/i);
    expect(convertMock).toHaveBeenCalledTimes(1);
    // Inspect the call args directly; expect.any(AbortSignal) trips
    // `no-unsafe-assignment` because matchers are typed `any`.
    const [paths, opts] = convertMock.mock.calls[0] as [
      string[],
      { target_format: string; jpeg_quality: number; alpha_handling: string },
    ];
    expect(paths).toEqual(["/tmp/a.png"]);
    expect(opts).toMatchObject({
      target_format: "jpeg",
      jpeg_quality: 85,
      alpha_handling: "flatten-white",
    });
  });

  it("renders the skipped list when run_job returns skips", async () => {
    convertMock.mockResolvedValueOnce({
      success_count: 1,
      skip_count: 1,
      skipped: [
        {
          source: "/tmp/bad.png",
          error: { kind: "UnsupportedFormat", message: "bad bytes" },
        },
      ],
      first_output_path: "/tmp/a.png",
      duration_ms: 1,
    });

    renderTool();
    await pickInto(["/tmp/a.png", "/tmp/bad.png"]);
    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await screen.findByText(/1 converted, 1 skipped/i);
    fireEvent.click(screen.getByText(/skipped files \(1\)/i));
    expect(screen.getByText(/bad\.png/)).toBeInTheDocument();
    expect(
      screen.getByText(/UnsupportedFormat: bad bytes/i),
    ).toBeInTheDocument();
  });

  it("preserves the staged list when the orchestrator errors", async () => {
    convertMock.mockRejectedValueOnce({
      kind: "Cancelled",
      message: "operation cancelled",
    });

    renderTool();
    await pickInto(["/tmp/a.png", "/tmp/b.jpg"]);
    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await screen.findByText(/operation cancelled/i);
    // Back in staging with both items intact.
    expect(screen.getByText(/staged \(2\)/i)).toBeInTheDocument();
  });
});
