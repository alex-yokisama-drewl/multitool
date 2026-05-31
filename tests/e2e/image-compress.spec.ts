import { expect, test } from "@playwright/test";

// Happy-path smoke for the Image Compress tool. Wrappers at `src/lib/`
// are aliased to `tests/e2e/mocks/` (vite.config.ts → e2eAliases):
// pickConvertibleImages returns three deliberately-named inputs
// (charlie.png, alpha.jpg, bravo.webp) so the staging UI exercises
// both the JPEG-preserving lane (.jpg → .jpg) and the WebP-redirect
// lane (.png → .webp; .webp stays .webp). compressImages streams
// Started → Succeeded per file at 30 ms with deterministic
// 2 MB → 1 MB (-50%) deltas so the done view's per-file size deltas
// land at known values. Failure paths are unit-covered in
// ImageCompress.test.tsx.

test("compresses a picked batch and surfaces per-file size deltas", async ({
  page,
}) => {
  await page.goto("/");

  // Image Compress tile lives in the Image section of the dashboard
  // (color: amber). Filter sections by their heading so the link match
  // doesn't collide with Video → Compress.
  const imageSection = page.locator("section").filter({
    has: page.getByRole("heading", { name: /^image$/i }),
  });
  await imageSection.getByRole("link", { name: /compress/i }).click();
  await expect(page).toHaveURL(/\/tools\/image-compress$/);

  // Idle → staging via the picker mock.
  await page.getByRole("button", { name: /^select image files$/i }).click();
  await expect(page.getByRole("button", { name: /^compress$/i })).toBeVisible();
  await expect(page.getByText(/staged \(3\)/i)).toBeVisible();

  // WebP-redirect banner reacts to the picked set. Mock has 1 PNG
  // (charlie.png); JPEG and WebP don't redirect.
  const banner = page.getByRole("status");
  await expect(banner).toBeVisible();
  await expect(banner).toContainText(/1 file will be saved as WebP/i);
  await expect(banner).toContainText(/PNG \(1\)/i);

  // Predicted output names show up under each staged tile per the
  // per-source strategy.
  await expect(page.getByText("→ charlie_compressed.webp")).toBeVisible();
  await expect(page.getByText("→ alpha_compressed.jpg")).toBeVisible();
  await expect(page.getByText("→ bravo_compressed.webp")).toBeVisible();

  // The single slider defaults to 0.8 (800 / 1000 — the "save for web"
  // sweet spot) and endpoints are labeled rather than numbered.
  const slider = page.getByRole("slider", { name: /^quality$/i });
  await expect(slider).toBeVisible();
  await expect(slider).toHaveValue("800");
  await expect(page.getByText(/^smaller file$/i)).toBeVisible();
  await expect(page.getByText(/^higher quality$/i)).toBeVisible();

  // Kick the compress.
  await page.getByRole("button", { name: /^compress$/i }).click();

  // Done state. Mock returns success_count = 3 and 2 MB → 1 MB per
  // file (-50%). Per-file deltas render three times.
  await expect(page.getByText(/3 compressed/i)).toBeVisible({ timeout: 5000 });
  await expect(page.getByText(/-50%/).first()).toBeVisible();
  expect(await page.getByText(/-50%/).count()).toBe(3);

  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /compress another batch/i }),
  ).toBeVisible();
});
