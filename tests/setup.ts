import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach } from "vitest";
import { cleanup } from "@testing-library/react";

// Vitest runs without `globals: true`, so Testing Library's `afterEach`
// auto-cleanup doesn't register itself. Wire it up once here.
afterEach(() => {
  cleanup();
});

// The prefs store is module-level state shared by every test in a file.
// Without this, a tool test that changes an option leaks that value into
// the next test's "renders its defaults" assertion. Resetting also leaves
// the store unhydrated, which is what keeps component tests from attempting
// an IPC write (see `src/lib/prefs.ts` → rule 2).
//
// Imported lazily on purpose. A top-level import here would instantiate the
// prefs module graph — and with it `@tauri-apps/api/core` — before a test
// file's `vi.mock` had a chance to register, so the mock would never take
// effect. Importing inside the hook defers it until after mock setup.
beforeEach(async () => {
  const { resetPrefsForTest } = await import("@/lib/prefs");
  resetPrefsForTest();
});
