import { expect, test } from "@playwright/test";

// Happy-path smoke for the Images → PDF tool. The wrappers at `src/lib/`
// get swapped for `tests/e2e/mocks/` (see vite.config.ts → e2eAliases):
// pickImageFiles returns three deliberately non-alphabetical paths
// (charlie / alpha / bravo), allowMediaPreview no-ops, and the convert
// mock streams one progress event per image at 30ms each then resolves.
// Failure paths are unit-covered in `ImagesToPdf.test.tsx`.

test("assembles staged images into a PDF and surfaces the done state", async ({
  page,
}) => {
  await page.goto("/");

  await page.getByRole("link", { name: /images → pdf/i }).click();

  // Idle → staging via the picker mock.
  await page.getByRole("button", { name: /add images/i }).click();
  await expect(page.getByRole("button", { name: /create pdf/i })).toBeVisible();

  // Filename-ascending sort puts alpha first; the preview drives off
  // the first item, so this exercises sort + preview together.
  await expect(page.getByTestId("output-preview")).toHaveText("alpha.pdf");
  await expect(page.getByText(/staged \(3\)/i)).toBeVisible();

  // Kick the conversion.
  await page.getByRole("button", { name: /create pdf/i }).click();

  // Done state reached. Mock returns the fixed output path; we just
  // assert the success-state affordances are visible.
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible({ timeout: 5000 });
  await expect(
    page.getByRole("button", { name: /convert another/i }),
  ).toBeVisible();
});
