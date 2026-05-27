import { expect, test } from "@playwright/test";

// Happy-path smoke for the Video Trimmer. Wrappers at `src/lib/` are
// aliased to `tests/e2e/mocks/` (vite.config.ts → e2eAliases):
// pickVideoFile returns a canned path; probeVideoDuration returns 12_000 ms;
// preparePreviewProxy resolves a fake proxy path (videoAssetUrl returns a
// tiny data: URL so the blob fetch succeeds); trimVideo resolves after
// ~30 ms with `{stem}_trimmed.{ext}`. Failure paths are unit-covered in
// VideoTrimmer.test.tsx.

test("trims a picked video file and surfaces the done state", async ({
  page,
}) => {
  // Two tools live under Video; go straight to the trimmer route.
  await page.goto("/#/tools/video-trimmer");

  // Idle → (preparing proxy) → picked.
  await page.getByRole("button", { name: /^select video file$/i }).click();
  await expect(page.getByRole("button", { name: /^trim$/i })).toBeVisible({
    timeout: 5000,
  });

  // Numeric inputs render at the mocked duration (12_000 ms), spanning the
  // whole clip by default.
  await expect(page.getByLabel(/^start$/i)).toHaveValue("00:00.000");
  await expect(page.getByLabel(/^end$/i)).toHaveValue("00:12.000");

  // The minimal player bar is present (play + seek + volume).
  await expect(page.getByRole("button", { name: /^play$/i })).toBeVisible();
  await expect(page.getByLabel(/^seek$/i)).toBeVisible();

  // Trim → done.
  await page.getByRole("button", { name: /^trim$/i }).click();
  await expect(page.getByText(/holiday_trimmed\.mov/i)).toBeVisible({
    timeout: 5000,
  });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /trim another/i }),
  ).toBeVisible();
});
