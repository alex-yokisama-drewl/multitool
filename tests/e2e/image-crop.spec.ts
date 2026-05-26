import { expect, test } from "@playwright/test";

// Happy-path smoke for the Image Crop tool. Wrappers at `src/lib/` are
// aliased to `tests/e2e/mocks/` (vite.config.ts → e2eAliases):
// pickRasterImage returns a canned path; imageAssetUrl returns a real 1×1
// PNG data URL so the preview's onLoad fires and the frame editor renders;
// cropImage resolves after 30 ms with `{stem}_cropped.{ext}`. Failure paths
// + the clamp/aspect-lock math are unit-covered (ImageCrop.test.tsx,
// cropGeometry.test.ts).

test("crops a picked image and surfaces the done state", async ({ page }) => {
  await page.goto("/#/tools/image-crop");

  // Idle → picked via the picker mock; the frame editor appears once the
  // (data-URL) preview image loads and natural dims are read.
  await page.getByRole("button", { name: /^select image$/i }).click();

  // The numeric inputs only render after the image loads — the mock image is
  // 1×1, so the frame defaults to the full image.
  await expect(page.getByLabel("Width")).toHaveValue("1");
  await expect(page.getByLabel("Height")).toHaveValue("1");
  await expect(page.getByLabel(/lock proportions/i)).toBeVisible();

  // Crop → done.
  await page.getByRole("button", { name: /^crop$/i }).click();
  await expect(page.getByText(/photo_cropped\.png/i)).toBeVisible({
    timeout: 5000,
  });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /crop another/i }),
  ).toBeVisible();
});
