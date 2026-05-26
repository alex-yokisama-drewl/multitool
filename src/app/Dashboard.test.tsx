import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { Dashboard } from "./Dashboard";

describe("Dashboard", () => {
  it("renders a tile for every registered tool", () => {
    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    );

    expect(
      screen.getByRole("link", { name: /pdf → images/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: /images → pdf/i }),
    ).toBeInTheDocument();
    // Three tools all named "Format Converter" (image, audio, video) —
    // qualify by section to keep the assertion specific.
    const imageSection = screen
      .getByRole("heading", { name: /^image$/i })
      .closest("section")!;
    const audioSection = screen
      .getByRole("heading", { name: /^audio$/i })
      .closest("section")!;
    const videoSection = screen
      .getByRole("heading", { name: /^video$/i })
      .closest("section")!;
    expect(
      within(imageSection).getByRole("link", { name: /format converter/i }),
    ).toBeInTheDocument();
    expect(
      within(audioSection).getByRole("link", { name: /format converter/i }),
    ).toBeInTheDocument();
    expect(
      within(videoSection).getByRole("link", { name: /format converter/i }),
    ).toBeInTheDocument();
    // Audio section now also carries the Trimmer.
    expect(
      within(audioSection).getByRole("link", { name: /trimmer/i }),
    ).toBeInTheDocument();
  });

  it("groups tiles into category sections in registry order", () => {
    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    );

    const pdfSection = screen
      .getByRole("heading", { name: /^pdf$/i })
      .closest("section")!;
    const imageSection = screen
      .getByRole("heading", { name: /^image$/i })
      .closest("section")!;
    const audioSection = screen
      .getByRole("heading", { name: /^audio$/i })
      .closest("section")!;
    const videoSection = screen
      .getByRole("heading", { name: /^video$/i })
      .closest("section")!;

    expect(
      within(pdfSection).getByRole("link", { name: /pdf → images/i }),
    ).toBeInTheDocument();
    expect(
      within(pdfSection).getByRole("link", { name: /images → pdf/i }),
    ).toBeInTheDocument();
    expect(
      within(imageSection).getByRole("link", {
        name: /format converter/i,
      }),
    ).toBeInTheDocument();
    expect(
      within(audioSection).getByRole("link", {
        name: /format converter/i,
      }),
    ).toBeInTheDocument();
    expect(
      within(audioSection).getByRole("link", { name: /trimmer/i }),
    ).toBeInTheDocument();
    // Two tiles under Audio now (Format Converter + Trimmer).
    expect(within(audioSection).getAllByRole("link")).toHaveLength(2);
    // Video section starts with just the Format Converter.
    expect(
      within(videoSection).getByRole("link", { name: /format converter/i }),
    ).toBeInTheDocument();
    expect(within(videoSection).getAllByRole("link")).toHaveLength(1);

    // PDF / Image / Audio / Video sections render in toolCategories order.
    const headings = screen.getAllByRole("heading", { level: 2 });
    expect(headings.map((h) => h.textContent)).toEqual([
      "PDF",
      "Image",
      "Audio",
      "Video",
    ]);
  });

  it("applies the registered tile color token to each tile", () => {
    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    );

    const pdfToImages = screen.getByRole("link", { name: /pdf → images/i });
    const imagesToPdf = screen.getByRole("link", { name: /images → pdf/i });
    const imageSection = screen
      .getByRole("heading", { name: /^image$/i })
      .closest("section")!;
    const audioSection = screen
      .getByRole("heading", { name: /^audio$/i })
      .closest("section")!;
    const videoSection = screen
      .getByRole("heading", { name: /^video$/i })
      .closest("section")!;
    const imageFormat = within(imageSection).getByRole("link", {
      name: /format converter/i,
    });
    const audioFormat = within(audioSection).getByRole("link", {
      name: /format converter/i,
    });
    const audioTrimmer = within(audioSection).getByRole("link", {
      name: /trimmer/i,
    });
    const videoFormat = within(videoSection).getByRole("link", {
      name: /format converter/i,
    });

    expect(pdfToImages.getAttribute("data-tile-color")).toBe("rose");
    expect(imagesToPdf.getAttribute("data-tile-color")).toBe("amber");
    expect(imageFormat.getAttribute("data-tile-color")).toBe("sky");
    expect(audioFormat.getAttribute("data-tile-color")).toBe("emerald");
    expect(audioTrimmer.getAttribute("data-tile-color")).toBe("violet");
    expect(videoFormat.getAttribute("data-tile-color")).toBe("teal");

    // Inline style binds the CSS var so the palette in globals.css is the
    // single source of truth for the actual color value.
    expect(pdfToImages.style.backgroundColor).toBe("var(--tile-rose)");
    expect(audioFormat.style.backgroundColor).toBe("var(--tile-emerald)");
    expect(audioTrimmer.style.backgroundColor).toBe("var(--tile-violet)");
    expect(videoFormat.style.backgroundColor).toBe("var(--tile-teal)");
  });
});
