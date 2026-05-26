import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { convertMock, pickVideoFilesMock, revealInFolderMock } = vi.hoisted(
  () => ({
    convertMock: vi.fn(),
    pickVideoFilesMock: vi.fn(),
    revealInFolderMock: vi.fn(),
  }),
);

vi.mock("@/lib/tools/videoFormatConverter", () => ({
  convertVideoFormat: convertMock,
}));
vi.mock("@/lib/system", () => ({
  pickVideoFiles: pickVideoFilesMock,
  revealInFolder: revealInFolderMock,
}));

import { VideoFormatConverter } from "./VideoFormatConverter";
import type { ConvertHooks, JobResult } from "@/lib/tools/videoFormatConverter";

function renderTool() {
  return render(
    <MemoryRouter>
      <VideoFormatConverter />
    </MemoryRouter>,
  );
}

async function pickInto(paths: string[]) {
  pickVideoFilesMock.mockResolvedValueOnce(paths);
  fireEvent.click(
    screen.getByRole("button", { name: /^select video files$/i }),
  );
  await screen.findByRole("button", { name: /^convert$/i });
}

describe("VideoFormatConverter", () => {
  beforeEach(() => {
    convertMock.mockReset();
    pickVideoFilesMock.mockReset();
    revealInFolderMock.mockReset();
  });

  it("renders the idle state with a Select video files button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /^select video files$/i }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("shows the three target-format radios with MP4 selected by default", async () => {
    renderTool();
    await pickInto(["/tmp/clip.mov"]);
    expect(screen.getByRole("radio", { name: /mp4/i })).toBeChecked();
    expect(screen.getByRole("radio", { name: /webm/i })).toBeInTheDocument();
    expect(
      screen.getByRole("radio", { name: /matroska/i }),
    ).toBeInTheDocument();
  });

  it("Select different videos REPLACES the staged batch (no merge)", async () => {
    renderTool();
    await pickInto(["/tmp/a.mp4", "/tmp/b.mov"]);
    expect(screen.getByText(/staged \(2\)/i)).toBeInTheDocument();

    pickVideoFilesMock.mockResolvedValueOnce(["/tmp/c.webm"]);
    fireEvent.click(
      screen.getByRole("button", { name: /select different videos/i }),
    );
    await waitFor(() => {
      expect(screen.getByText(/staged \(1\)/i)).toBeInTheDocument();
    });
    expect(screen.queryByText("a.mp4")).not.toBeInTheDocument();
    expect(screen.queryByText("b.mov")).not.toBeInTheDocument();
  });

  it("forwards picked paths + opts to convertVideoFormat after switching target", async () => {
    renderTool();
    await pickInto(["/tmp/holiday.mov"]);

    fireEvent.click(screen.getByRole("radio", { name: /webm/i }));

    convertMock.mockResolvedValueOnce({
      success_count: 1,
      skip_count: 0,
      skipped: [],
      first_output_path: "/tmp/holiday_converted.webm",
      duration_ms: 1234,
    });

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await waitFor(() => {
      expect(convertMock).toHaveBeenCalledTimes(1);
    });
    const [paths, opts] = convertMock.mock.calls[0] as [
      string[],
      Record<string, unknown>,
    ];
    expect(paths).toEqual(["/tmp/holiday.mov"]);
    expect(opts).toEqual({ target_format: "webm" });
  });

  it("renders an error envelope when convert fails and preserves staged paths", async () => {
    renderTool();
    await pickInto(["/tmp/holiday.mov"]);
    convertMock.mockRejectedValueOnce({
      kind: "Cancelled",
      message: "operation cancelled",
    });

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Cancelled");
    expect(alert).toHaveTextContent(/operation cancelled/i);
    expect(
      screen.getByRole("button", { name: /^convert$/i }),
    ).toBeInTheDocument();
  });

  it("shows the done summary with skip count and skipped details", async () => {
    renderTool();
    await pickInto(["/tmp/a.mp4", "/tmp/b.mp4"]);
    convertMock.mockResolvedValueOnce({
      success_count: 1,
      skip_count: 1,
      skipped: [
        {
          source: "/tmp/b.mp4",
          error: {
            kind: "ProcessingFailed",
            message: "ffmpeg exited with status 1",
          },
        },
      ],
      first_output_path: "/tmp/a_converted.mp4",
      duration_ms: 5000,
    });

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await screen.findByText(/1 converted, 1 skipped/i);
    fireEvent.click(screen.getByText(/skipped files \(1\)/i));
    expect(screen.getByText("b.mp4")).toBeInTheDocument();
    expect(
      screen.getByText(/ProcessingFailed: ffmpeg exited with status 1/i),
    ).toBeInTheDocument();
  });

  it("renders a per-file progress bar reflecting file-progress fractions", async () => {
    renderTool();
    await pickInto(["/tmp/clip.mov"]);

    // Convert resolves eventually; in the meantime we stream a Started
    // event then a file-progress at 42% to drive the progressbar.
    let resolveConvert: (result: JobResult) => void = () => undefined;
    convertMock.mockImplementation(
      (_paths: string[], _opts: unknown, hooks: ConvertHooks) => {
        hooks.onProgress?.({
          kind: "started",
          index: 0,
          total: 1,
          source: "/tmp/clip.mov",
        });
        hooks.onProgress?.({
          kind: "file-progress",
          index: 0,
          total: 1,
          source: "/tmp/clip.mov",
          fraction: 0.42,
        });
        return new Promise<JobResult>((res) => {
          resolveConvert = res;
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    const bar = await screen.findByRole("progressbar");
    // aria-label is computed from the same `state.current.fraction` value
    // as the progress bar's `value` prop — assert on it for stability
    // across radix internal-attribute changes.
    await waitFor(() => {
      expect(bar.getAttribute("aria-label")).toContain("42%");
    });

    // Finish the job so the test exits cleanly.
    resolveConvert({
      success_count: 1,
      skip_count: 0,
      skipped: [],
      first_output_path: "/tmp/clip_converted.mp4",
      duration_ms: 1000,
    });
    await screen.findByText(/1 converted/i);
  });
});
