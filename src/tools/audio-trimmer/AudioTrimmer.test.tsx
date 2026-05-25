import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { pickAudioFileMock } = vi.hoisted(() => ({
  pickAudioFileMock: vi.fn(),
}));

vi.mock("@/lib/system", () => ({
  pickAudioFile: pickAudioFileMock,
}));

import { AudioTrimmer } from "./AudioTrimmer";

function renderTool() {
  return render(
    <MemoryRouter>
      <AudioTrimmer />
    </MemoryRouter>,
  );
}

describe("AudioTrimmer (scaffold)", () => {
  beforeEach(() => {
    pickAudioFileMock.mockReset();
  });

  it("renders the idle state with a Select audio file button", () => {
    renderTool();
    expect(
      screen.getByRole("button", { name: /^select audio file$/i }),
    ).toBeInTheDocument();
  });

  it("moves to picked state and shows the file name + Pick different file", async () => {
    pickAudioFileMock.mockResolvedValueOnce("/tmp/song.wav");
    renderTool();
    fireEvent.click(
      screen.getByRole("button", { name: /^select audio file$/i }),
    );
    await waitFor(() => {
      expect(screen.getByText("song.wav")).toBeInTheDocument();
    });
    expect(
      screen.getByRole("button", { name: /^pick different file$/i }),
    ).toBeInTheDocument();
  });

  it("leaves idle untouched when the picker is cancelled", async () => {
    pickAudioFileMock.mockResolvedValueOnce(null);
    renderTool();
    fireEvent.click(
      screen.getByRole("button", { name: /^select audio file$/i }),
    );
    await waitFor(() => {
      expect(pickAudioFileMock).toHaveBeenCalled();
    });
    expect(
      screen.getByRole("button", { name: /^select audio file$/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^pick different file$/i }),
    ).not.toBeInTheDocument();
  });
});
