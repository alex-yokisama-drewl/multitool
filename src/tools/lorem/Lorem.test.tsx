import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Lorem } from "./Lorem";

function renderTool() {
  return render(
    <MemoryRouter>
      <Lorem />
    </MemoryRouter>,
  );
}

// Paragraphs are rendered as separate <p> elements, so the underlying text
// state can't be read off `.textContent` (which strips the blank lines).
// Read the children directly instead.
function renderedParagraphs() {
  return Array.from(
    screen.getByTestId("lorem-output").querySelectorAll("p"),
  ).map((p) => p.textContent ?? "");
}

describe("Lorem", () => {
  let writeText: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
  });

  it("renders 5 paragraphs on mount", () => {
    renderTool();
    expect(renderedParagraphs()).toHaveLength(5);
    expect(renderedParagraphs().every((p) => p.length > 0)).toBe(true);
  });

  it("Regenerate produces different text", () => {
    renderTool();
    const before = renderedParagraphs().join("\n\n");
    fireEvent.click(screen.getByRole("button", { name: /regenerate/i }));
    const after = renderedParagraphs().join("\n\n");
    expect(after).not.toBe(before);
  });

  it("Copy writes the rendered text (paragraphs joined by blank lines) and shows a 'Copied' affordance", async () => {
    renderTool();
    const expected = renderedParagraphs().join("\n\n");
    fireEvent.click(screen.getByRole("button", { name: /^copy$/i }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(expected);
    });
    await screen.findByRole("button", { name: /^copied$/i });
  });

  it("falls back to 'Copy failed' when clipboard.writeText rejects", async () => {
    writeText.mockRejectedValueOnce(new Error("denied"));
    renderTool();
    fireEvent.click(screen.getByRole("button", { name: /^copy$/i }));
    await screen.findByRole("button", { name: /copy failed/i });
  });
});
