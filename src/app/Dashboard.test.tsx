import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
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
  });
});
