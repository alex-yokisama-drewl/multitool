import { expect, test } from "@playwright/test";

// Happy-path smoke for the Video Compress tool. Wrappers at `src/lib/`
// are aliased to `tests/e2e/mocks/` (vite.config.ts → e2eAliases):
// pickVideoFile returns a canned path; probeVideoDuration returns
// 12_000 ms; preparePreviewProxy resolves a fake proxy path
// (videoAssetUrl returns a tiny data: URL so the blob fetch succeeds);
// compressVideo resolves after ~30 ms with `{stem}_compressed.{ext}`.
// Failure paths are unit-covered in VideoCompress.test.tsx.

test("compresses a picked video file and surfaces the done state", async ({
  page,
}) => {
  await page.goto("/#/tools/video-compress");

  // Idle → (preparing proxy) → picked.
  await page.getByRole("button", { name: /^select video file$/i }).click();
  await expect(page.getByRole("button", { name: /^compress$/i })).toBeVisible({
    timeout: 5000,
  });

  // The quality slider is present and defaults to the midpoint
  // (500 / 1000 — the value the Rust side rounds to the per-codec
  // CRF midpoint).
  const slider = page.getByRole("slider", { name: /^quality$/i });
  await expect(slider).toBeVisible();
  await expect(slider).toHaveValue("500");

  // Endpoints are labeled — no number / no codec name, intentionally.
  await expect(page.getByText(/^smaller file$/i)).toBeVisible();
  await expect(page.getByText(/^higher quality$/i)).toBeVisible();

  // Compress → done.
  await page.getByRole("button", { name: /^compress$/i }).click();
  await expect(page.getByText(/holiday_compressed\.mov/i)).toBeVisible({
    timeout: 5000,
  });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /compress another/i }),
  ).toBeVisible();
});
