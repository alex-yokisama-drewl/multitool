import { test, expect } from "@playwright/test";

// Happy-path smoke for the PDF → Images tool. The wrappers at `src/lib/` get
// swapped for `tests/e2e/mocks/` (see vite.config.ts → e2eAliases): the
// picker returns a hardcoded path, the conversion mock streams 3 progress
// events at 30ms each then resolves. Failure paths are unit-covered in
// `PdfToImages.test.tsx`; this lane is intentionally a smoke.

test("converts a PDF and surfaces the success state with Open output folder", async ({
  page,
}) => {
  await page.goto("/");

  await page.getByRole("link", { name: /pdf → images/i }).click();

  await page.getByRole("button", { name: /choose pdf/i }).click();

  await expect(page.getByRole("button", { name: /^convert$/i })).toBeVisible();
  await page.getByRole("button", { name: /^convert$/i }).click();

  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible({ timeout: 5000 });

  await expect(
    page.getByRole("button", { name: /convert another/i }),
  ).toBeVisible();
});
