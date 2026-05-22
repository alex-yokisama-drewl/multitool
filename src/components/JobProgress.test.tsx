import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { JobProgress } from "./JobProgress";

describe("JobProgress", () => {
  it("renders the label prefix with N / total", () => {
    render(
      <JobProgress current={2} total={3} label="page" onCancel={vi.fn()} />,
    );
    expect(screen.getByText("page 2 / 3")).toBeInTheDocument();
  });

  it("omits the prefix when no label is supplied", () => {
    render(<JobProgress current={2} total={3} onCancel={vi.fn()} />);
    expect(screen.getByText("2 / 3")).toBeInTheDocument();
  });

  it("renders 'starting…' when total is 0", () => {
    render(
      <JobProgress current={0} total={0} label="page" onCancel={vi.fn()} />,
    );
    expect(screen.getByText("starting…")).toBeInTheDocument();
  });

  it("computes the percent value on the progress bar from current / total", () => {
    const { container } = render(
      <JobProgress current={1} total={4} label="page" onCancel={vi.fn()} />,
    );
    // The shadcn `<Progress>` indicator encodes the value as a transform on
    // its indicator child: `translateX(-${100 - value}%)`. For 1/4 = 25%
    // that's translateX(-75%).
    const indicator = container.querySelector(
      '[data-slot="progress-indicator"]',
    );
    expect(indicator).not.toBeNull();
    expect((indicator as HTMLElement).style.transform).toBe("translateX(-75%)");
  });

  it("invokes onCancel when the Cancel button is clicked", () => {
    const onCancel = vi.fn();
    render(
      <JobProgress current={1} total={3} label="page" onCancel={onCancel} />,
    );
    fireEvent.click(screen.getByRole("button", { name: /cancel/i }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
