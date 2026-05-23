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
    expect(
      screen.getByRole("link", { name: /image format converter/i }),
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

    expect(
      within(pdfSection).getByRole("link", { name: /pdf → images/i }),
    ).toBeInTheDocument();
    expect(
      within(pdfSection).getByRole("link", { name: /images → pdf/i }),
    ).toBeInTheDocument();
    expect(
      within(imageSection).getByRole("link", {
        name: /image format converter/i,
      }),
    ).toBeInTheDocument();

    // PDF section comes before Image section per toolCategories order.
    const headings = screen.getAllByRole("heading", { level: 2 });
    expect(headings.map((h) => h.textContent)).toEqual(["PDF", "Image"]);
  });
});
