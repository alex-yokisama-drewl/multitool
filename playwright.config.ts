import { defineConfig, devices } from "@playwright/test";

// Per DECISIONS.md "Playwright (not WebdriverIO/tauri-driver) for e2e":
// Playwright drives the Vite dev server, not a packaged Tauri binary. Tauri
// IPC is mocked at the `src/lib/` wrapper boundary inside individual specs so
// happy-path UI coverage works without a desktop runtime. If we later need
// real desktop-shell coverage we add a second e2e lane on `tauri-driver` /
// WebdriverIO without ripping this out.

const PORT = 1420;
const BASE_URL = `http://localhost:${PORT}`;

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "github" : "list",

  use: {
    baseURL: BASE_URL,
    trace: "on-first-retry",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  webServer: {
    command: "pnpm dev",
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
