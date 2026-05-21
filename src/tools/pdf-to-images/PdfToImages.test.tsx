import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";

const { convertMock, pickPdfFileMock, revealInFolderMock } = vi.hoisted(() => ({
  convertMock: vi.fn(),
  pickPdfFileMock: vi.fn(),
  revealInFolderMock: vi.fn(),
}));

vi.mock("@/lib/tools/pdfToImages", () => ({
  convertPdfToImages: convertMock,
}));
vi.mock("@/lib/system", () => ({
  pickPdfFile: pickPdfFileMock,
  revealInFolder: revealInFolderMock,
}));

import { PdfToImages } from "./PdfToImages";

function renderTool() {
  return render(
    <MemoryRouter>
      <PdfToImages />
    </MemoryRouter>,
  );
}

async function pickAndShowForm(path = "/tmp/in.pdf") {
  pickPdfFileMock.mockResolvedValueOnce(path);
  fireEvent.click(screen.getByRole("button", { name: /choose pdf/i }));
  await screen.findByRole("button", { name: /^convert$/i });
}

describe("PdfToImages", () => {
  beforeEach(() => {
    convertMock.mockReset();
    pickPdfFileMock.mockReset();
    revealInFolderMock.mockReset();
  });

  it("defaults to PNG format and DPI 150 once a file is picked", async () => {
    renderTool();
    await pickAndShowForm();

    expect(screen.getByLabelText("PNG")).toBeChecked();
    expect(screen.getByLabelText("JPEG")).not.toBeChecked();
    expect(screen.getByLabelText(/dpi/i)).toHaveValue(150);
  });

  it("passes the selected JPEG + DPI 300 through to convertPdfToImages", async () => {
    renderTool();
    await pickAndShowForm("/tmp/doc.pdf");

    convertMock.mockResolvedValueOnce({
      output_dir: "/tmp/doc_pages",
      page_count: 1,
      duration_ms: 1,
    });

    fireEvent.click(screen.getByLabelText("JPEG"));
    fireEvent.change(screen.getByLabelText(/dpi/i), {
      target: { value: "300" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await waitFor(() => {
      expect(convertMock).toHaveBeenCalledTimes(1);
    });
    expect(convertMock).toHaveBeenCalledWith(
      "/tmp/doc.pdf",
      { format: "jpeg", dpi: 300 },
      expect.objectContaining({
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        signal: expect.any(AbortSignal),
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        onProgress: expect.any(Function),
      }),
    );
  });

  it("renders streaming progress in the per-page counter", async () => {
    renderTool();
    await pickAndShowForm();

    interface Result {
      output_dir: string;
      page_count: number;
      duration_ms: number;
    }
    let resolveJob: ((result: Result) => void) | undefined;
    convertMock.mockImplementation(
      (
        _path: string,
        _opts: { format: string; dpi: number },
        hooks: { onProgress?: (p: { page: number; total: number }) => void },
      ) => {
        hooks.onProgress?.({ page: 1, total: 3 });
        hooks.onProgress?.({ page: 2, total: 3 });
        return new Promise<Result>((resolve) => {
          resolveJob = resolve;
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await screen.findByText(/page 2 \/ 3/i);

    resolveJob?.({
      output_dir: "/tmp/in_pages",
      page_count: 3,
      duration_ms: 1,
    });
    await screen.findByRole("button", { name: /open output folder/i });
  });

  it("shows the error envelope message when the IPC wrapper rejects", async () => {
    renderTool();
    await pickAndShowForm();

    convertMock.mockRejectedValueOnce({
      kind: "Encrypted",
      message: "password-protected PDF",
    });

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Encrypted");
    expect(alert).toHaveTextContent("password-protected PDF");
  });

  it("aborts the in-flight job's AbortSignal when Cancel is clicked", async () => {
    renderTool();
    await pickAndShowForm();

    let capturedSignal: AbortSignal | undefined;
    convertMock.mockImplementation(
      (
        _path: string,
        _opts: { format: string; dpi: number },
        hooks: { signal?: AbortSignal },
      ) => {
        capturedSignal = hooks.signal;
        return new Promise(() => {
          /* never resolves — we just want to inspect the signal */
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));
    await screen.findByRole("button", { name: /cancel/i });

    expect(capturedSignal?.aborted).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    expect(capturedSignal?.aborted).toBe(true);
  });
});
