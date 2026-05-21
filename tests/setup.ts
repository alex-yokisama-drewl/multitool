import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// Vitest runs without `globals: true`, so Testing Library's `afterEach`
// auto-cleanup doesn't register itself. Wire it up once here.
afterEach(() => {
  cleanup();
});
