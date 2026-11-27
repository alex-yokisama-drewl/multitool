import { test, expect } from "@playwright/test";

// Happy-path smoke for remembered tool options.
//
// A page reload is the browser-side stand-in for quitting and relaunching
// the app: the prefs store is dropped and re-hydrated from the settings
// document, which `tests/e2e/mocks/settings.ts` backs with sessionStorage.
// That is the one half of this feature an e2e lane can actually observe —
// the directory memory lives behind the OS file dialog, which is stubbed
// out here, so it stays unit-covered in `src/lib/lastDirectory.test.ts`.

async function openPdfToImages(page: import("@playwright/test").Page) {
  await page.goto("/");
  await page.getByRole("link", { name: /pdf → images/i }).click();
  await page.getByRole("button", { name: /choose pdf/i }).click();
  await expect(page.getByRole("button", { name: /^convert$/i })).toBeVisible();
}

test("remembers the chosen format and DPI across a reload", async ({
  page,
}) => {
  await openPdfToImages(page);

  await expect(page.getByLabel("PNG")).toBeChecked();
  await expect(page.getByLabel(/dpi/i)).toHaveValue("150");

  await page.getByLabel("JPEG").check();
  await page.getByLabel(/dpi/i).fill("300");

  // Outlive the write debounce before dropping the page.
  await expect(page.getByLabel(/dpi/i)).toHaveValue("300");
  await page.waitForTimeout(600);

  await openPdfToImages(page);

  await expect(page.getByLabel("JPEG")).toBeChecked();
  await expect(page.getByLabel(/dpi/i)).toHaveValue("300");
});

test("keeps the chosen options after converting", async ({ page }) => {
  await openPdfToImages(page);

  await page.getByLabel("JPEG").check();
  await page.getByRole("button", { name: /^convert$/i }).click();

  await expect(
    page.getByRole("button", { name: /convert another/i }),
  ).toBeVisible({ timeout: 5000 });
  await page.getByRole("button", { name: /convert another/i }).click();

  await page.getByRole("button", { name: /choose pdf/i }).click();
  await expect(page.getByLabel("JPEG")).toBeChecked();
});
