import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { convertMock, pickConvertibleAudioMock, revealInFolderMock } =
  vi.hoisted(() => ({
    convertMock: vi.fn(),
    pickConvertibleAudioMock: vi.fn(),
    revealInFolderMock: vi.fn(),
  }));

vi.mock("@/lib/tools/audioFormatConverter", () => ({
  convertAudioFormat: convertMock,
}));
vi.mock("@/lib/system", () => ({
  pickConvertibleAudio: pickConvertibleAudioMock,
  revealInFolder: revealInFolderMock,
}));

import { AudioFormatConverter } from "./AudioFormatConverter";

function renderTool() {
  return render(
    <MemoryRouter>
      <AudioFormatConverter />
    </MemoryRouter>,
  );
}

async function pickInto(paths: string[]) {
  pickConvertibleAudioMock.mockResolvedValueOnce(paths);
  fireEvent.click(
    screen.getByRole("button", { name: /^select audio files$/i }),
  );
  await screen.findByRole("button", { name: /^convert$/i });
}

describe("AudioFormatConverter", () => {
  beforeEach(() => {
    convertMock.mockReset();
    pickConvertibleAudioMock.mockReset();
    revealInFolderMock.mockReset();
  });

  it("renders the idle state with a Select audio files button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /^select audio files$/i }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("shows the MP3 bitrate field when MP3 is the target, hides it for other formats", async () => {
    renderTool();
    await pickInto(["/tmp/song.wav"]);
    // Default target is MP3 → bitrate visible.
    expect(screen.getByLabelText(/mp3 bitrate/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/ogg quality/i)).not.toBeInTheDocument();

    // Switch to OGG → quality visible, MP3 bitrate gone.
    fireEvent.click(screen.getByRole("radio", { name: /ogg vorbis/i }));
    expect(screen.queryByLabelText(/mp3 bitrate/i)).not.toBeInTheDocument();
    expect(screen.getByLabelText(/ogg quality/i)).toBeInTheDocument();

    // Switch to WAV → bit-depth radio group visible.
    fireEvent.click(screen.getByRole("radio", { name: /^wav$/i }));
    expect(screen.getByText(/wav bit depth/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/ogg quality/i)).not.toBeInTheDocument();
  });

  it("Select different audio REPLACES the staged batch (no merge)", async () => {
    renderTool();
    await pickInto(["/tmp/a.wav", "/tmp/b.mp3"]);
    expect(screen.getByText(/staged \(2\)/i)).toBeInTheDocument();

    pickConvertibleAudioMock.mockResolvedValueOnce(["/tmp/c.flac"]);
    fireEvent.click(
      screen.getByRole("button", { name: /select different audio/i }),
    );
    await waitFor(() => {
      expect(screen.getByText(/staged \(1\)/i)).toBeInTheDocument();
    });
    expect(screen.queryByText("a.wav")).not.toBeInTheDocument();
    expect(screen.queryByText("b.mp3")).not.toBeInTheDocument();
  });

  it("forwards picked paths + opts to convertAudioFormat with clamped bitrate", async () => {
    renderTool();
    await pickInto(["/tmp/song.wav"]);

    // Tweak the MP3 bitrate to something below the min (96) — should clamp.
    const bitrate = screen.getByLabelText(/mp3 bitrate/i);
    fireEvent.change(bitrate, { target: { value: "10" } });

    convertMock.mockResolvedValueOnce({
      success_count: 1,
      skip_count: 0,
      skipped: [],
      first_output_path: "/tmp/song.mp3",
      duration_ms: 42,
    });

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await waitFor(() => {
      expect(convertMock).toHaveBeenCalledTimes(1);
    });
    const [paths, opts] = convertMock.mock.calls[0] as [
      string[],
      Record<string, unknown>,
    ];
    expect(paths).toEqual(["/tmp/song.wav"]);
    expect(opts).toMatchObject({
      target_format: "mp3",
      mp3_bitrate_kbps: 96, // clamped from 10 → 96 (MP3_BITRATE_MIN)
      channels: "source",
    });
  });

  it("renders an error envelope when convert fails and preserves staged paths", async () => {
    renderTool();
    await pickInto(["/tmp/song.wav"]);
    convertMock.mockRejectedValueOnce({
      kind: "Cancelled",
      message: "operation cancelled",
    });

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    // After the rejected promise resolves, state goes back to staging
    // with the error envelope and the alert div renders.
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Cancelled");
    expect(alert).toHaveTextContent(/operation cancelled/i);
    // Still in staging — Convert button is back.
    expect(
      screen.getByRole("button", { name: /^convert$/i }),
    ).toBeInTheDocument();
  });

  it("shows the done summary with skip count and skipped details", async () => {
    renderTool();
    await pickInto(["/tmp/a.wav", "/tmp/b.wav"]);
    convertMock.mockResolvedValueOnce({
      success_count: 1,
      skip_count: 1,
      skipped: [
        {
          source: "/tmp/b.wav",
          error: { kind: "UnsupportedFormat", message: "bad bytes" },
        },
      ],
      first_output_path: "/tmp/a.mp3",
      duration_ms: 50,
    });

    fireEvent.click(screen.getByRole("button", { name: /^convert$/i }));

    await screen.findByText(/1 converted, 1 skipped/i);
    // Click the details to verify the skipped file shows.
    fireEvent.click(screen.getByText(/skipped files \(1\)/i));
    expect(screen.getByText("b.wav")).toBeInTheDocument();
    expect(
      screen.getByText(/UnsupportedFormat: bad bytes/i),
    ).toBeInTheDocument();
  });
});
