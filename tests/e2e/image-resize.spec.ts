import { expect, test } from "@playwright/test";

// Happy-path smoke for the Image Resize tool. Wrappers at `src/lib/` are
// aliased to `tests/e2e/mocks/` (vite.config.ts → e2eAliases):
// pickRasterImage returns a canned path; imageAssetUrl returns a real 1×1
// PNG data URL so the preview's onLoad fires and the form renders;
// resizeImage resolves after 30 ms with `{stem}_resized.{ext}`. The lock
// math, conditional UI, and aspect-mismatch gating are unit-covered
// (ImageResize.test.tsx).

test("resizes a picked image and surfaces the done state", async ({ page }) => {
  await page.goto("/#/tools/image-resize");

  // Idle → picked via the picker mock; the form appears once the
  // (data-URL) preview image loads and natural dims are read.
  await page.getByRole("button", { name: /^select image$/i }).click();

  // The numeric inputs only render after the image loads — the mock image
  // is 1×1, so W/H both default to 1 and the lock defaults ON.
  await expect(page.getByLabel("Width")).toHaveValue("1");
  await expect(page.getByLabel("Height")).toHaveValue("1");
  await expect(page.getByLabel(/lock aspect ratio/i)).toBeChecked();
  // 1×1 → target aspect matches source → Fit picker hidden on the
  // happy-path entry view.
  await expect(page.getByText(/^fit$/i)).toBeHidden();

  // Resize → done.
  await page.getByRole("button", { name: /^resize$/i }).click();
  await expect(page.getByText(/photo_resized\.png/i)).toBeVisible({
    timeout: 5000,
  });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /resize another/i }),
  ).toBeVisible();
});
