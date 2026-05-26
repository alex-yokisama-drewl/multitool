import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";

const { extractMock, pickVideoFileMock, revealInFolderMock } = vi.hoisted(
  () => ({
    extractMock: vi.fn(),
    pickVideoFileMock: vi.fn(),
    revealInFolderMock: vi.fn(),
  }),
);

vi.mock("@/lib/tools/audioExtractor", () => ({
  extractAudio: extractMock,
}));
vi.mock("@/lib/system", () => ({
  pickVideoFile: pickVideoFileMock,
  revealInFolder: revealInFolderMock,
}));

import { AudioExtractor } from "./AudioExtractor";
import type { JobResult, Progress } from "./types";

function renderTool() {
  return render(
    <MemoryRouter>
      <AudioExtractor />
    </MemoryRouter>,
  );
}

async function pickAndShowForm(path = "/tmp/holiday.mov") {
  pickVideoFileMock.mockResolvedValueOnce(path);
  fireEvent.click(screen.getByRole("button", { name: /select video file/i }));
  await screen.findByRole("button", { name: /extract audio/i });
}

describe("AudioExtractor", () => {
  beforeEach(() => {
    extractMock.mockReset();
    pickVideoFileMock.mockReset();
    revealInFolderMock.mockReset();
  });

  it("starts in idle and shows the picker button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /select video file/i }),
    ).toBeInTheDocument();
  });

  it("passes the picked path to extractAudio with hooks", async () => {
    renderTool();
    await pickAndShowForm("/tmp/clip.mp4");

    extractMock.mockResolvedValueOnce({
      track_count: 1,
      outputs: ["/tmp/clip_audio.mp3"],
      duration_ms: 1,
    } satisfies JobResult);

    fireEvent.click(screen.getByRole("button", { name: /extract audio/i }));

    await waitFor(() => {
      expect(extractMock).toHaveBeenCalledTimes(1);
    });
    expect(extractMock).toHaveBeenCalledWith(
      "/tmp/clip.mp4",
      expect.objectContaining({
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        signal: expect.any(AbortSignal),
        // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
        onProgress: expect.any(Function),
      }),
    );
  });

  it("renders single-track progress without a 'Track N of M' label", async () => {
    renderTool();
    await pickAndShowForm();

    let resolveJob: ((result: JobResult) => void) | undefined;
    extractMock.mockImplementation(
      (
        _path: string,
        hooks: { onProgress?: (p: Progress) => void },
      ) => {
        hooks.onProgress?.({ kind: "started", index: 0, total: 1 });
        hooks.onProgress?.({
          kind: "file-progress",
          index: 0,
          total: 1,
          fraction: 0.5,
        });
        return new Promise<JobResult>((resolve) => {
          resolveJob = resolve;
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /extract audio/i }));

    // Progress bar appears.
    await screen.findByLabelText(/extraction progress:/i);
    // No "Track 1 of 1" because single-track sources don't label.
    expect(screen.queryByText(/track \d+ of \d+/i)).not.toBeInTheDocument();

    resolveJob?.({
      track_count: 1,
      outputs: ["/tmp/holiday_audio.mp3"],
      duration_ms: 1,
    });
    await screen.findByRole("button", { name: /open output folder/i });
  });

  it("renders 'Track N of M' for multi-track sources", async () => {
    renderTool();
    await pickAndShowForm("/tmp/concert.mkv");

    let resolveJob: ((result: JobResult) => void) | undefined;
    extractMock.mockImplementation(
      (
        _path: string,
        hooks: { onProgress?: (p: Progress) => void },
      ) => {
        hooks.onProgress?.({ kind: "started", index: 1, total: 3 });
        return new Promise<JobResult>((resolve) => {
          resolveJob = resolve;
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /extract audio/i }));
    await screen.findByText(/track 2 of 3/i);

    resolveJob?.({
      track_count: 3,
      outputs: [
        "/tmp/concert_audio_1.mp3",
        "/tmp/concert_audio_2.mp3",
        "/tmp/concert_audio_3.mp3",
      ],
      duration_ms: 1,
    });
    await screen.findByRole("button", { name: /open output folder/i });

    // Done view lists each output filename.
    expect(screen.getByText("concert_audio_1.mp3")).toBeInTheDocument();
    expect(screen.getByText("concert_audio_2.mp3")).toBeInTheDocument();
    expect(screen.getByText("concert_audio_3.mp3")).toBeInTheDocument();
  });

  it("shows the error envelope and returns to the picked-file view", async () => {
    renderTool();
    await pickAndShowForm();

    extractMock.mockRejectedValueOnce({
      kind: "ProcessingFailed",
      message: "no audio streams",
    });

    fireEvent.click(screen.getByRole("button", { name: /extract audio/i }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("ProcessingFailed");
    expect(alert).toHaveTextContent("no audio streams");
    // Still in picked view (retry available without re-picking).
    expect(
      screen.getByRole("button", { name: /extract audio/i }),
    ).toBeInTheDocument();
  });

  it("aborts the in-flight job's AbortSignal when Cancel is clicked", async () => {
    renderTool();
    await pickAndShowForm();

    let capturedSignal: AbortSignal | undefined;
    extractMock.mockImplementation(
      (_path: string, hooks: { signal?: AbortSignal }) => {
        capturedSignal = hooks.signal;
        return new Promise(() => {
          /* never resolves */
        });
      },
    );

    fireEvent.click(screen.getByRole("button", { name: /extract audio/i }));
    await screen.findByRole("button", { name: /cancel/i });

    expect(capturedSignal?.aborted).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    expect(capturedSignal?.aborted).toBe(true);
  });

  it("reveals the first output path when Open output folder is clicked", async () => {
    renderTool();
    await pickAndShowForm();
    extractMock.mockResolvedValueOnce({
      track_count: 2,
      outputs: ["/tmp/a_audio_1.mp3", "/tmp/a_audio_2.mp3"],
      duration_ms: 1,
    } satisfies JobResult);

    fireEvent.click(screen.getByRole("button", { name: /extract audio/i }));
    const openBtn = await screen.findByRole("button", {
      name: /open output folder/i,
    });
    fireEvent.click(openBtn);
    expect(revealInFolderMock).toHaveBeenCalledWith("/tmp/a_audio_1.mp3");
  });
});
