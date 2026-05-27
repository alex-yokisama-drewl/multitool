import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  pickAudioFileMock,
  allowMediaPreviewMock,
  revealInFolderMock,
  loadAudioPreviewMock,
  createPreviewPlayerMock,
  trimAudioMock,
  previewStopMock,
} = vi.hoisted(() => ({
  pickAudioFileMock: vi.fn(),
  allowMediaPreviewMock: vi.fn(),
  revealInFolderMock: vi.fn(),
  loadAudioPreviewMock: vi.fn(),
  createPreviewPlayerMock: vi.fn(),
  trimAudioMock: vi.fn(),
  previewStopMock: vi.fn(),
}));

vi.mock("@/lib/system", () => ({
  pickAudioFile: pickAudioFileMock,
  allowMediaPreview: allowMediaPreviewMock,
  revealInFolder: revealInFolderMock,
}));
vi.mock("@/lib/audioPreview", () => ({
  loadAudioPreview: loadAudioPreviewMock,
  createPreviewPlayer: createPreviewPlayerMock,
}));
vi.mock("@/lib/tools/audioTrimmer", () => ({
  trimAudio: trimAudioMock,
}));
// The Waveform component pokes at canvas + ResizeObserver — both
// missing in jsdom and irrelevant to the state-machine assertions
// here. Replace it with a marker div so the rest of the picked-view
// renders.
vi.mock("./Waveform", () => ({
  Waveform: () => <div data-testid="waveform" />,
}));

import { AudioTrimmer, FADE_PRESET_MS } from "./AudioTrimmer";

function renderTool() {
  return render(
    <MemoryRouter>
      <AudioTrimmer />
    </MemoryRouter>,
  );
}

const mockSource = () => ({
  durationMs: 10_000,
  peaks: [],
  audioBuffer: {} as unknown as AudioBuffer,
  audioContext: {} as unknown as AudioContext,
});

async function pickAndLoad(path = "/tmp/song.wav") {
  pickAudioFileMock.mockResolvedValueOnce(path);
  allowMediaPreviewMock.mockResolvedValueOnce(undefined);
  loadAudioPreviewMock.mockResolvedValueOnce(mockSource());
  fireEvent.click(screen.getByRole("button", { name: /^select audio file$/i }));
  // Wait for the picked-view fields to land.
  await screen.findByRole("button", { name: /^trim$/i });
}

describe("AudioTrimmer", () => {
  beforeEach(() => {
    pickAudioFileMock.mockReset();
    allowMediaPreviewMock.mockReset();
    revealInFolderMock.mockReset();
    loadAudioPreviewMock.mockReset();
    createPreviewPlayerMock.mockReset();
    trimAudioMock.mockReset();
    previewStopMock.mockReset();
    createPreviewPlayerMock.mockImplementation(() => ({
      stop: previewStopMock,
    }));
  });

  it("renders the idle state with a Select audio file button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /^select audio file$/i }),
    ).toBeInTheDocument();
  });

  it("after picking, calls allowMediaPreview and renders the picked view", async () => {
    renderTool();
    await pickAndLoad("/tmp/song.wav");
    expect(allowMediaPreviewMock).toHaveBeenCalledWith(["/tmp/song.wav"]);
    expect(loadAudioPreviewMock).toHaveBeenCalledWith("/tmp/song.wav");
    // Picked view renders the waveform + numeric inputs at the defaults.
    expect(screen.getByTestId("waveform")).toBeInTheDocument();
    // Defaults: start = 0 ms = "00:00.000"; end = duration (10_000 ms) = "00:10.000".
    expect(screen.getByLabelText(/^start$/i)).toHaveValue("00:00.000");
    expect(screen.getByLabelText(/^end$/i)).toHaveValue("00:10.000");
  });

  it("silently clamps end >= start + 1 ms (no error alert, Trim stays enabled)", async () => {
    renderTool();
    await pickAndLoad();
    // Try to push End to the same value as Start (or below). The clamp
    // should silently bump End back to startMs + 1.
    fireEvent.change(screen.getByLabelText(/^end$/i), {
      target: { value: "00:00.000" },
    });
    fireEvent.blur(screen.getByLabelText(/^end$/i));
    // No alert about the range — the silent clamp pins End at 1 ms.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByLabelText(/^end$/i)).toHaveValue("00:00.001");
    // Trim stays enabled.
    expect(screen.getByRole("button", { name: /^trim$/i })).not.toBeDisabled();
  });

  it("silently clamps start <= end - 1 ms when start is pushed past end", async () => {
    renderTool();
    await pickAndLoad();
    // Push start way above end (10_000 ms).
    fireEvent.change(screen.getByLabelText(/^start$/i), {
      target: { value: "00:30" },
    });
    fireEvent.blur(screen.getByLabelText(/^start$/i));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    // Start is clamped to endMs - 1 = 9_999 ms.
    expect(screen.getByLabelText(/^start$/i)).toHaveValue("00:09.999");
  });

  it("re-formats numeric input on blur and rejects malformed values", async () => {
    renderTool();
    await pickAndLoad();
    const start = screen.getByLabelText(/^start$/i);

    // Malformed value: input snaps back to the prior valid value (0).
    fireEvent.change(start, { target: { value: "garbage" } });
    fireEvent.blur(start);
    expect(start).toHaveValue("00:00.000");

    // Valid input within the 10s duration: "00:02" → "00:02.000".
    fireEvent.change(start, { target: { value: "00:02" } });
    fireEvent.blur(start);
    expect(start).toHaveValue("00:02.000");

    // Above max: clamps via the silent end-1 ms rule (endMs=10_000 → max 9_999).
    fireEvent.change(start, { target: { value: "01:30" } });
    fireEvent.blur(start);
    expect(start).toHaveValue("00:09.999");
  });

  it("forwards path + opts including fade presets when Trim is clicked", async () => {
    renderTool();
    await pickAndLoad("/tmp/song.wav");
    fireEvent.click(screen.getByLabelText(/fade in/i));
    fireEvent.click(screen.getByLabelText(/fade out/i));

    trimAudioMock.mockResolvedValueOnce({
      output: "/tmp/song_trimmed.wav",
      warnings: [],
      duration_ms: 42,
    });

    fireEvent.click(screen.getByRole("button", { name: /^trim$/i }));

    await waitFor(() => {
      expect(trimAudioMock).toHaveBeenCalledTimes(1);
    });
    const [path, opts] = trimAudioMock.mock.calls[0] as [
      string,
      Record<string, number>,
    ];
    expect(path).toBe("/tmp/song.wav");
    expect(opts).toEqual({
      start_ms: 0,
      end_ms: 10_000,
      fade_in_ms: FADE_PRESET_MS,
      fade_out_ms: FADE_PRESET_MS,
    });
  });

  it("shows the done summary with the output path + Open output folder button", async () => {
    renderTool();
    await pickAndLoad("/tmp/song.wav");
    trimAudioMock.mockResolvedValueOnce({
      output: "/tmp/song_trimmed.wav",
      warnings: [
        "fade-in/out exceeded trim window of 10000 ms; clamped each to 5000 ms",
      ],
      duration_ms: 50,
    });

    fireEvent.click(screen.getByRole("button", { name: /^trim$/i }));

    await screen.findByText(/song_trimmed\.wav/i);
    expect(screen.getByText(/fade-in\/out exceeded/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /open output folder/i }),
    ).toBeInTheDocument();

    // Open output folder routes through revealInFolder.
    fireEvent.click(
      screen.getByRole("button", { name: /open output folder/i }),
    );
    await waitFor(() => {
      expect(revealInFolderMock).toHaveBeenCalledWith("/tmp/song_trimmed.wav");
    });
  });

  it("surfaces the error envelope and returns to the picked view on failure", async () => {
    renderTool();
    await pickAndLoad();
    trimAudioMock.mockRejectedValueOnce({
      kind: "Cancelled",
      message: "operation cancelled",
    });
    fireEvent.click(screen.getByRole("button", { name: /^trim$/i }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Cancelled");
    expect(alert).toHaveTextContent(/operation cancelled/i);
    // Still in picked — Trim button is back.
    expect(screen.getByRole("button", { name: /^trim$/i })).toBeInTheDocument();
  });

  it("Play toggle creates and stops the preview player", async () => {
    renderTool();
    await pickAndLoad();
    fireEvent.click(screen.getByRole("button", { name: /^play$/i }));
    expect(createPreviewPlayerMock).toHaveBeenCalledTimes(1);
    const [, , opts] = createPreviewPlayerMock.mock.calls[0] as [
      unknown,
      unknown,
      Record<string, number>,
    ];
    expect(opts).toMatchObject({
      startMs: 0,
      endMs: 10_000,
      fadeInMs: 0,
      fadeOutMs: 0,
    });
    // Button now reads "Stop" (icon-only, exposed via aria-label).
    expect(screen.getByRole("button", { name: /^stop$/i })).toBeInTheDocument();

    // Toggle off.
    fireEvent.click(screen.getByRole("button", { name: /^stop$/i }));
    expect(previewStopMock).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /^play$/i })).toBeInTheDocument();
  });

  it("changing start/end/fade while preview is playing stops the preview", async () => {
    renderTool();
    await pickAndLoad();
    fireEvent.click(screen.getByRole("button", { name: /^play$/i }));
    expect(screen.getByRole("button", { name: /^stop$/i })).toBeInTheDocument();

    // Editing Start stops the preview.
    fireEvent.change(screen.getByLabelText(/^start$/i), {
      target: { value: "00:01.000" },
    });
    fireEvent.blur(screen.getByLabelText(/^start$/i));
    expect(previewStopMock).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: /^play$/i })).toBeInTheDocument();

    // Play again → toggling a fade checkbox stops it.
    fireEvent.click(screen.getByRole("button", { name: /^play$/i }));
    expect(screen.getByRole("button", { name: /^stop$/i })).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/fade in/i));
    expect(previewStopMock).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: /^play$/i })).toBeInTheDocument();
  });

  it("surfaces a load failure as an idle-state error", async () => {
    renderTool();
    pickAudioFileMock.mockResolvedValueOnce("/tmp/song.wav");
    allowMediaPreviewMock.mockResolvedValueOnce(undefined);
    loadAudioPreviewMock.mockRejectedValueOnce({
      kind: "UnsupportedFormat",
      message: "bad bytes",
    });

    fireEvent.click(
      screen.getByRole("button", { name: /^select audio file$/i }),
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("UnsupportedFormat");
    expect(alert).toHaveTextContent(/bad bytes/i);
    // Back to idle — the Select button is still there.
    expect(
      screen.getByRole("button", { name: /^select audio file$/i }),
    ).toBeInTheDocument();
  });

  it("Pick different file re-runs the picker from the picked view", async () => {
    renderTool();
    await pickAndLoad("/tmp/a.wav");
    pickAudioFileMock.mockResolvedValueOnce("/tmp/b.mp3");
    allowMediaPreviewMock.mockResolvedValueOnce(undefined);
    loadAudioPreviewMock.mockResolvedValueOnce({
      durationMs: 20_000,
      peaks: [],
      audioBuffer: {} as unknown as AudioBuffer,
      audioContext: {} as unknown as AudioContext,
    });
    fireEvent.click(
      screen.getByRole("button", { name: /pick different file/i }),
    );
    // After re-load, end-time should reflect the new duration.
    await waitFor(() => {
      expect(screen.getByLabelText(/^end$/i)).toHaveValue("00:20.000");
    });
  });
});
