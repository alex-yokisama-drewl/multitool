import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  cropMock,
  pickRasterImageMock,
  allowMediaPreviewMock,
  revealInFolderMock,
  imageAssetUrlMock,
} = vi.hoisted(() => ({
  cropMock: vi.fn(),
  pickRasterImageMock: vi.fn(),
  allowMediaPreviewMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  imageAssetUrlMock: vi.fn((path: string) => `asset://${path}`),
}));

vi.mock("@/lib/tools/imageCrop", () => ({ cropImage: cropMock }));
vi.mock("@/lib/system", () => ({
  pickRasterImage: pickRasterImageMock,
  allowMediaPreview: allowMediaPreviewMock,
  revealInFolder: revealInFolderMock,
  imageAssetUrl: imageAssetUrlMock,
}));

// CropFrame uses ResizeObserver, which jsdom lacks. The crop math is covered
// by cropGeometry.test.ts; here we only need the observer to be constructible.
class ResizeObserverStub {
  observe() {
    /* no-op: jsdom has no layout to observe */
  }
  unobserve() {
    /* no-op */
  }
  disconnect() {
    /* no-op */
  }
}
vi.stubGlobal("ResizeObserver", ResizeObserverStub);

import { ImageCrop } from "./ImageCrop";

const PATH = "/tmp/photo.png";
const okResult = { output: "/tmp/photo_cropped.png", duration_ms: 9 };

function renderTool() {
  return render(
    <MemoryRouter>
      <ImageCrop />
    </MemoryRouter>,
  );
}

// Pick an image and fire its load with the given natural dimensions, landing
// in the interactive picked state with the frame at full image.
async function pickAndLoad(width: number, height: number) {
  pickRasterImageMock.mockResolvedValueOnce(PATH);
  allowMediaPreviewMock.mockResolvedValueOnce(undefined);
  fireEvent.click(screen.getByRole("button", { name: /^select image$/i }));

  const img = await screen.findByAltText("Crop preview");
  Object.defineProperty(img, "naturalWidth", {
    value: width,
    configurable: true,
  });
  Object.defineProperty(img, "naturalHeight", {
    value: height,
    configurable: true,
  });
  fireEvent.load(img);
  await screen.findByRole("button", { name: /^crop$/i });
}

function cropArgs() {
  return cropMock.mock.calls[0] as [
    string,
    { x: number; y: number; width: number; height: number },
    unknown,
  ];
}

describe("ImageCrop", () => {
  beforeEach(() => {
    cropMock.mockReset();
    pickRasterImageMock.mockReset();
    allowMediaPreviewMock.mockReset();
    revealInFolderMock.mockReset();
  });

  it("renders the idle state with a Select image button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /^select image$/i }),
    ).toBeInTheDocument();
    expect(screen.queryByAltText("Crop preview")).not.toBeInTheDocument();
  });

  it("stays idle when the picker is cancelled (null)", async () => {
    renderTool();
    pickRasterImageMock.mockResolvedValueOnce(null);
    fireEvent.click(screen.getByRole("button", { name: /^select image$/i }));
    // No image preview ever appears.
    await Promise.resolve();
    expect(screen.queryByAltText("Crop preview")).not.toBeInTheDocument();
  });

  it("defaults the frame to the full image and forwards it to cropImage", async () => {
    cropMock.mockResolvedValueOnce(okResult);
    renderTool();
    await pickAndLoad(200, 100);

    fireEvent.click(screen.getByRole("button", { name: /^crop$/i }));
    await screen.findByText(/done/i);

    const [path, rect] = cropArgs();
    expect(path).toBe(PATH);
    expect(rect).toEqual({ x: 0, y: 0, width: 200, height: 100 });
  });

  it("forwards an edited width (no lock leaves height untouched)", async () => {
    cropMock.mockResolvedValueOnce(okResult);
    renderTool();
    await pickAndLoad(200, 100);

    const widthInput = screen.getByLabelText("Width");
    fireEvent.change(widthInput, { target: { value: "80" } });
    fireEvent.blur(widthInput);

    fireEvent.click(screen.getByRole("button", { name: /^crop$/i }));
    await screen.findByText(/done/i);

    const [, rect] = cropArgs();
    expect(rect).toEqual({ x: 0, y: 0, width: 80, height: 100 });
  });

  it("locks proportions at toggle time so a width edit drives height", async () => {
    cropMock.mockResolvedValueOnce(okResult);
    renderTool();
    await pickAndLoad(200, 100); // ratio 2:1

    fireEvent.click(screen.getByLabelText(/lock proportions/i));
    const widthInput = screen.getByLabelText("Width");
    fireEvent.change(widthInput, { target: { value: "80" } });
    fireEvent.blur(widthInput);

    // Height field should reflect the derived value.
    expect(screen.getByLabelText("Height")).toHaveValue(40);

    fireEvent.click(screen.getByRole("button", { name: /^crop$/i }));
    await screen.findByText(/done/i);

    const [, rect] = cropArgs();
    expect(rect).toEqual({ x: 0, y: 0, width: 80, height: 40 });
  });

  it("renders the error envelope and keeps the picked image on failure", async () => {
    cropMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "crop rectangle does not intersect the image",
    });
    renderTool();
    await pickAndLoad(200, 100);

    fireEvent.click(screen.getByRole("button", { name: /^crop$/i }));

    expect(
      await screen.findByText(/does not intersect the image/i),
    ).toBeInTheDocument();
    // Still on the picked view — image preview present for a retry.
    expect(screen.getByAltText("Crop preview")).toBeInTheDocument();
  });

  it("Cancel aborts the in-flight crop's signal", async () => {
    let captured: AbortSignal | undefined;
    cropMock.mockImplementation(
      (_p: string, _r: unknown, hooks: { signal?: AbortSignal }) => {
        captured = hooks.signal;
        return new Promise(() => {
          // never resolves — stays in the cropping state
        });
      },
    );
    renderTool();
    await pickAndLoad(200, 100);

    fireEvent.click(screen.getByRole("button", { name: /^crop$/i }));
    fireEvent.click(await screen.findByRole("button", { name: /cancel/i }));

    expect(captured?.aborted).toBe(true);
  });
});
