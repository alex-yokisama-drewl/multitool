import { expect, test } from "@playwright/test";

// Happy-path smoke for the Image Format Converter. Wrappers at
// `src/lib/` are aliased to `tests/e2e/mocks/` (vite.config.ts →
// e2eAliases). pickConvertibleImages returns three deliberately-named
// inputs; convertImageFormat streams Started → Succeeded events per
// image at 30 ms each and resolves with success_count = 3.
// Failure paths are unit-covered in ImageFormatConverter.test.tsx.

test("converts staged images and surfaces the done state", async ({ page }) => {
  await page.goto("/");

  await page.getByRole("link", { name: /image format converter/i }).click();

  // Idle → staging via the picker mock.
  await page.getByRole("button", { name: /add images/i }).click();
  await expect(page.getByRole("button", { name: /^convert$/i })).toBeVisible();
  await expect(page.getByText(/staged \(3\)/i)).toBeVisible();

  // Default target = PNG, default jpeg_quality / alpha_handling fields
  // hidden. Switch to JPEG to exercise the conditional field.
  await page.getByLabel("JPEG").click();
  await expect(page.getByLabel(/jpeg quality/i)).toBeVisible();
  await expect(page.getByText(/alpha handling/i)).toBeVisible();

  // Kick the conversion.
  await page.getByRole("button", { name: /^convert$/i }).click();

  // Done state. Mock returns success_count = 3, skip_count = 0.
  await expect(page.getByText(/3 converted/i)).toBeVisible({ timeout: 5000 });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /convert another/i }),
  ).toBeVisible();
});
