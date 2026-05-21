import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { Dashboard } from "./Dashboard";

describe("Dashboard", () => {
  it("renders the empty-state placeholder when no tools are registered", () => {
    render(
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>,
    );

    expect(
      screen.getByRole("heading", { name: /no tools yet/i }),
    ).toBeInTheDocument();
  });
});
