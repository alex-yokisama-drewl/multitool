import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  resizeMock,
  pickRasterImageMock,
  allowMediaPreviewMock,
  revealInFolderMock,
  imageAssetUrlMock,
  getRasterFormatsMock,
} = vi.hoisted(() => ({
  resizeMock: vi.fn(),
  pickRasterImageMock: vi.fn(),
  allowMediaPreviewMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  imageAssetUrlMock: vi.fn((path: string) => `asset://${path}`),
  getRasterFormatsMock: vi.fn(),
}));

vi.mock("@/lib/tools/imageResize", () => ({ resizeImage: resizeMock }));
vi.mock("@/lib/system", () => ({
  pickRasterImage: pickRasterImageMock,
  allowMediaPreview: allowMediaPreviewMock,
  revealInFolder: revealInFolderMock,
  imageAssetUrl: imageAssetUrlMock,
}));
vi.mock("@/lib/imageFormats", () => ({
  getRasterFormats: getRasterFormatsMock,
}));

import { ImageResize } from "./ImageResize";

const PNG_PATH = "/tmp/photo.png";
const JPG_PATH = "/tmp/photo.jpg";
const okResult = { output: "/tmp/photo_resized.png", duration_ms: 12 };

// Minimal stand-in for `RasterFormatDescriptor[]`. Two entries are enough to
// drive the Transparent-option gating (one alpha-capable, one not).
const FORMATS = [
  { id: "png", name: "PNG", extensions: ["png"], supports_alpha: true },
  {
    id: "jpeg",
    name: "JPEG",
    extensions: ["jpg", "jpeg"],
    supports_alpha: false,
  },
];

function renderTool() {
  return render(
    <MemoryRouter>
      <ImageResize />
    </MemoryRouter>,
  );
}

// Pick an image at `path` and fire its load with the given natural
// dimensions, landing in the interactive picked state with W/H populated.
async function pickAndLoad(path: string, width: number, height: number) {
  pickRasterImageMock.mockResolvedValueOnce(path);
  allowMediaPreviewMock.mockResolvedValueOnce(undefined);
  fireEvent.click(screen.getByRole("button", { name: /^select image$/i }));

  const img = await screen.findByAltText("Resize preview");
  Object.defineProperty(img, "naturalWidth", {
    value: width,
    configurable: true,
  });
  Object.defineProperty(img, "naturalHeight", {
    value: height,
    configurable: true,
  });
  fireEvent.load(img);
  // Wait for inputs to appear (gated on `dims`).
  await screen.findByLabelText(/^width$/i);
}

function widthInput() {
  return screen.getByLabelText(/^width$/i);
}
function heightInput() {
  return screen.getByLabelText(/^height$/i);
}
function lockCheckbox() {
  return screen.getByLabelText(/lock aspect ratio/i);
}

describe("ImageResize", () => {
  beforeEach(() => {
    resizeMock.mockReset();
    pickRasterImageMock.mockReset();
    allowMediaPreviewMock.mockReset();
    revealInFolderMock.mockReset();
    getRasterFormatsMock.mockReset();
    getRasterFormatsMock.mockResolvedValue(FORMATS);
  });

  it("renders idle with a Select image button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /^select image$/i }),
    ).toBeInTheDocument();
    expect(screen.queryByAltText("Resize preview")).not.toBeInTheDocument();
  });

  it("stays idle when the picker is cancelled (null)", async () => {
    renderTool();
    pickRasterImageMock.mockResolvedValueOnce(null);
    fireEvent.click(screen.getByRole("button", { name: /^select image$/i }));
    // After the rejection of pick, the component is still showing the
    // Select button (no preview rendered).
    await waitFor(() => {
      expect(screen.queryByAltText("Resize preview")).not.toBeInTheDocument();
    });
  });

  it("populates W/H from natural dims and starts with the lock ON", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50);

    expect(widthInput()).toHaveValue(100);
    expect(heightInput()).toHaveValue(50);
    expect(lockCheckbox()).toBeChecked();
    // Target aspect == source aspect → fit selector hidden.
    expect(screen.queryByText(/^fit$/i)).not.toBeInTheDocument();
  });

  it("locks aspect: editing width updates height proportionally", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50); // 2:1 ratio

    fireEvent.change(widthInput(), { target: { value: "300" } });
    fireEvent.blur(widthInput());

    expect(widthInput()).toHaveValue(300);
    expect(heightInput()).toHaveValue(150);
  });

  it("keeps Fit hidden while target aspect matches source, regardless of lock", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50); // 2:1 source

    // Lock ON, natural dims → match → hidden.
    expect(screen.queryByText(/^fit$/i)).not.toBeInTheDocument();

    // Toggle lock OFF without changing W/H — still 100×50, still matches.
    fireEvent.click(lockCheckbox());
    expect(screen.queryByText(/^fit$/i)).not.toBeInTheDocument();

    // Lock ON, then scale proportionally — still 2:1, still matches.
    fireEvent.click(lockCheckbox());
    fireEvent.change(widthInput(), { target: { value: "300" } });
    fireEvent.blur(widthInput());
    expect(heightInput()).toHaveValue(150); // confirmed proportional
    expect(screen.queryByText(/^fit$/i)).not.toBeInTheDocument();
  });

  it("reveals Fit when the target aspect differs from the source", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50); // 2:1 source

    // Break aspect: lock OFF, type a non-matching width.
    fireEvent.click(lockCheckbox());
    fireEvent.change(widthInput(), { target: { value: "300" } });
    fireEvent.blur(widthInput());

    expect(screen.getByText(/^fit$/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/stretch/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/^contain$/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/cover/i)).toBeInTheDocument();
  });

  it("keeps Fit visible after re-locking to a non-natural aspect", async () => {
    // Source 100×50 (2:1). User goes 100×100 with lock off, then locks
    // — captured ratio is 1:1 ≠ source aspect, so the fit picker must
    // stay visible (the lock alone doesn't restore aspect parity).
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50);

    fireEvent.click(lockCheckbox()); // lock OFF
    fireEvent.change(heightInput(), { target: { value: "100" } });
    fireEvent.blur(heightInput());
    expect(screen.getByText(/^fit$/i)).toBeInTheDocument();

    // Lock back ON, capturing 1:1.
    fireEvent.click(lockCheckbox());
    expect(lockCheckbox()).toBeChecked();
    // Aspect still mismatched (target 1:1 vs source 2:1) → Fit stays.
    expect(screen.getByText(/^fit$/i)).toBeInTheDocument();
  });

  it("reveals Background only when aspect differs AND fit=Contain", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50);

    // Break aspect → fit shows; default fit is Contain → bg shows.
    fireEvent.click(lockCheckbox());
    fireEvent.change(widthInput(), { target: { value: "300" } });
    fireEvent.blur(widthInput());
    expect(screen.getByText(/^background$/i)).toBeInTheDocument();

    // Switch to Stretch → bg hides.
    fireEvent.click(screen.getByLabelText(/stretch/i));
    expect(screen.queryByText(/^background$/i)).not.toBeInTheDocument();

    // Switch back to Contain → bg returns.
    fireEvent.click(screen.getByLabelText(/^contain$/i));
    expect(screen.getByText(/^background$/i)).toBeInTheDocument();
  });

  it("shows the Transparent background option only when source supports alpha", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50);
    // Break aspect to expose the bg picker.
    fireEvent.click(lockCheckbox());
    fireEvent.change(widthInput(), { target: { value: "300" } });
    fireEvent.blur(widthInput());
    // PNG supports alpha → Transparent visible.
    expect(screen.getByLabelText(/transparent/i)).toBeInTheDocument();
  });

  it("hides Transparent background for JPEG sources", async () => {
    renderTool();
    await pickAndLoad(JPG_PATH, 100, 50);
    // Break aspect to expose the bg picker.
    fireEvent.click(lockCheckbox());
    fireEvent.change(widthInput(), { target: { value: "300" } });
    fireEvent.blur(widthInput());
    // JPEG can't carry alpha → Transparent hidden.
    expect(screen.queryByLabelText(/transparent/i)).not.toBeInTheDocument();
    expect(screen.getByLabelText(/^white$/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/^black$/i)).toBeInTheDocument();
  });

  it("Reset restores W and H to the natural dimensions", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50);

    fireEvent.change(widthInput(), { target: { value: "800" } });
    fireEvent.blur(widthInput());
    expect(widthInput()).toHaveValue(800);

    fireEvent.click(screen.getByRole("button", { name: /reset/i }));
    expect(widthInput()).toHaveValue(100);
    expect(heightInput()).toHaveValue(50);
  });

  it("forwards width/height/fit/background to resizeImage on submit", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50);

    // Break aspect first so the Fit picker is visible, then pick Stretch.
    fireEvent.click(lockCheckbox());
    fireEvent.change(widthInput(), { target: { value: "300" } });
    fireEvent.blur(widthInput());
    fireEvent.change(heightInput(), { target: { value: "200" } });
    fireEvent.blur(heightInput());
    fireEvent.click(screen.getByLabelText(/stretch/i));

    resizeMock.mockResolvedValueOnce(okResult);
    fireEvent.click(screen.getByRole("button", { name: /^resize$/i }));

    await waitFor(() => {
      expect(resizeMock).toHaveBeenCalledTimes(1);
    });
    const [path, opts, hooks] = resizeMock.mock.calls[0] as [
      string,
      unknown,
      { signal?: AbortSignal },
    ];
    expect(path).toBe(PNG_PATH);
    expect(opts).toEqual({
      width: 300,
      height: 200,
      fit: "stretch",
      // Background isn't read by Stretch but is still forwarded as-is —
      // PNG default is "transparent".
      background: "transparent",
    });
    expect(hooks.signal).toBeInstanceOf(AbortSignal);
  });

  it("renders an error envelope and preserves the picked path on failure", async () => {
    renderTool();
    await pickAndLoad(JPG_PATH, 100, 50);

    // Force Contain + Transparent, which the backend rejects for JPEG.
    fireEvent.click(lockCheckbox());
    // (Transparent isn't visible for JPEG, but we can force the state via
    // a backend error path: assume something else went wrong.)
    resizeMock.mockRejectedValueOnce({
      kind: "UnsupportedFormat",
      message:
        "target format 'jpg' does not support alpha; choose a white or black background for Contain",
    });
    fireEvent.click(screen.getByRole("button", { name: /^resize$/i }));

    await screen.findByRole("alert");
    expect(screen.getByText(/UnsupportedFormat/)).toBeInTheDocument();
    // The picked file is still on screen so the user can retry.
    expect(screen.getByAltText("Resize preview")).toBeInTheDocument();
  });

  it("Cancel during resize aborts the AbortSignal", async () => {
    renderTool();
    await pickAndLoad(PNG_PATH, 100, 50);

    let capturedSignal: AbortSignal | undefined;
    resizeMock.mockImplementation(
      (_p: string, _o: unknown, hooks: { signal?: AbortSignal }) => {
        capturedSignal = hooks.signal;
        // Stay pending; the test will check the abort flag after Cancel.
        return new Promise(() => undefined);
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /^resize$/i }));
    await screen.findByRole("button", { name: /^cancel$/i });
    fireEvent.click(screen.getByRole("button", { name: /^cancel$/i }));

    expect(capturedSignal?.aborted).toBe(true);
  });
});
