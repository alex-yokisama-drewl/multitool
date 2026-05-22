import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  convertMock,
  pickImageFilesMock,
  allowImagePreviewMock,
  revealInFolderMock,
  convertFileSrcMock,
} = vi.hoisted(() => ({
  convertMock: vi.fn(),
  pickImageFilesMock: vi.fn(),
  allowImagePreviewMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  convertFileSrcMock: vi.fn((path: string) => `asset://${path}`),
}));

vi.mock("@/lib/tools/imagesToPdf", () => ({
  convertImagesToPdf: convertMock,
}));
vi.mock("@/lib/system", () => ({
  pickImageFiles: pickImageFilesMock,
  allowImagePreview: allowImagePreviewMock,
  revealInFolder: revealInFolderMock,
}));
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: convertFileSrcMock,
}));

import { ImagesToPdf } from "./ImagesToPdf";

function renderTool() {
  return render(
    <MemoryRouter>
      <ImagesToPdf />
    </MemoryRouter>,
  );
}

/** Drive the picker → staging transition with a given path list. The
 * mocks deliver `paths` from pickImageFiles and resolve allowImagePreview;
 * await the "Create PDF" button to know we're settled in `staging`. */
async function pickInto(paths: string[]) {
  pickImageFilesMock.mockResolvedValueOnce(paths);
  allowImagePreviewMock.mockResolvedValueOnce(undefined);
  fireEvent.click(screen.getByRole("button", { name: /add images/i }));
  await screen.findByRole("button", { name: /create pdf/i });
}

describe("ImagesToPdf", () => {
  beforeEach(() => {
    convertMock.mockReset();
    pickImageFilesMock.mockReset();
    allowImagePreviewMock.mockReset();
    revealInFolderMock.mockReset();
  });

  it("renders the idle state with an Add images button by default", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /add images/i }),
    ).toBeInTheDocument();
    // No grid yet — only the picker affordance.
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("transitions to staging when the picker returns paths", async () => {
    renderTool();
    await pickInto(["/tmp/a.png", "/tmp/b.jpg"]);

    expect(screen.getByRole("list", { name: /staged images/i })).toBeVisible();
    expect(screen.getByText(/staged \(2\)/i)).toBeInTheDocument();
    expect(allowImagePreviewMock).toHaveBeenCalledWith([
      "/tmp/a.png",
      "/tmp/b.jpg",
    ]);
  });

  it("stays in idle when the picker is cancelled (null)", async () => {
    renderTool();
    pickImageFilesMock.mockResolvedValueOnce(null);
    fireEvent.click(screen.getByRole("button", { name: /add images/i }));

    await waitFor(() => {
      expect(pickImageFilesMock).toHaveBeenCalledTimes(1);
    });
    expect(
      screen.queryByRole("button", { name: /create pdf/i }),
    ).not.toBeInTheDocument();
    expect(allowImagePreviewMock).not.toHaveBeenCalled();
  });

  it("sorts staged items by filename ascending, and the output preview tracks the first", async () => {
    renderTool();
    // Pick in deliberately non-alphabetical order; the brief says initial
    // order on each batch is filename ascending.
    await pickInto(["/photos/zeta.png", "/photos/alpha.jpg"]);

    const preview = screen.getByTestId("output-preview");
    expect(preview).toHaveTextContent("alpha.pdf");
  });

  it("removes an item via its × button and returns to idle when the list empties", async () => {
    renderTool();
    await pickInto(["/tmp/only.png"]);

    fireEvent.click(screen.getByRole("button", { name: /remove only\.png/i }));

    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: /create pdf/i }),
      ).not.toBeInTheDocument();
    });
    // Back to the idle picker.
    expect(
      screen.getByRole("button", { name: /add images/i }),
    ).toBeInTheDocument();
  });

  it("defaults page-size to auto-fit and forwards a different choice to the wrapper", async () => {
    renderTool();
    await pickInto(["/tmp/a.png"]);

    expect(screen.getByLabelText(/auto-fit/i)).toBeChecked();
    expect(screen.getByLabelText("A4")).not.toBeChecked();

    convertMock.mockResolvedValueOnce({
      output_path: "/tmp/a.pdf",
      page_count: 1,
      duration_ms: 1,
    });

    fireEvent.click(screen.getByLabelText("A4"));
    fireEvent.click(screen.getByRole("button", { name: /create pdf/i }));

    await waitFor(() => {
      expect(convertMock).toHaveBeenCalledTimes(1);
    });
    expect(convertMock).toHaveBeenCalledWith(
      ["/tmp/a.png"],
      { page_size: "a4" },
      expect.objectContaining({
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        signal: expect.any(AbortSignal),
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        onProgress: expect.any(Function),
      }),
    );
  });

  it("renders streaming progress as image N / total", async () => {
    renderTool();
    await pickInto(["/tmp/a.png", "/tmp/b.jpg", "/tmp/c.webp"]);

    interface Result {
      output_path: string;
      page_count: number;
      duration_ms: number;
    }
    let resolveJob: ((result: Result) => void) | undefined;
    convertMock.mockImplementation(
      (
        _paths: string[],
        _opts: { page_size: string },
        hooks: {
          onProgress?: (p: { image: number; total: number }) => void;
        },
      ) => {
        hooks.onProgress?.({ image: 1, total: 3 });
        hooks.onProgress?.({ image: 2, total: 3 });
        return new Promise<Result>((resolve) => {
          resolveJob = resolve;
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /create pdf/i }));

    await screen.findByText(/image 2 \/ 3/i);

    resolveJob?.({
      output_path: "/tmp/a.pdf",
      page_count: 3,
      duration_ms: 1,
    });
    await screen.findByRole("button", { name: /open output folder/i });
  });

  it("folds an error envelope back into the staging view with the items preserved", async () => {
    renderTool();
    await pickInto(["/tmp/a.png", "/tmp/b.jpg"]);

    convertMock.mockRejectedValueOnce({
      kind: "FileNotFound",
      message: "file not found: /tmp/a.png",
    });

    fireEvent.click(screen.getByRole("button", { name: /create pdf/i }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("FileNotFound");
    expect(alert).toHaveTextContent("/tmp/a.png");
    // The staging grid + Create PDF should still be visible — the brief's
    // "kept in staging state with the existing list preserved" rule.
    expect(
      screen.getByRole("button", { name: /create pdf/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/staged \(2\)/i)).toBeInTheDocument();
  });

  it("aborts the in-flight job's AbortSignal when Cancel is clicked", async () => {
    renderTool();
    await pickInto(["/tmp/a.png"]);

    let capturedSignal: AbortSignal | undefined;
    convertMock.mockImplementation(
      (
        _paths: string[],
        _opts: { page_size: string },
        hooks: { signal?: AbortSignal },
      ) => {
        capturedSignal = hooks.signal;
        return new Promise(() => {
          /* never resolves — we just want to inspect the signal */
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /create pdf/i }));
    await screen.findByRole("button", { name: /cancel/i });

    expect(capturedSignal?.aborted).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    expect(capturedSignal?.aborted).toBe(true);
  });

  it("opens the output folder on done via revealInFolder(output_path)", async () => {
    renderTool();
    await pickInto(["/tmp/a.png"]);

    convertMock.mockResolvedValueOnce({
      output_path: "/tmp/a.pdf",
      page_count: 1,
      duration_ms: 1,
    });
    fireEvent.click(screen.getByRole("button", { name: /create pdf/i }));

    const openButton = await screen.findByRole("button", {
      name: /open output folder/i,
    });
    fireEvent.click(openButton);
    expect(revealInFolderMock).toHaveBeenCalledWith("/tmp/a.pdf");
  });
});
