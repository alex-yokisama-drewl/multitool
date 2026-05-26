import { expect, test } from "@playwright/test";

// Happy-path smoke for the Video Format Converter. Wrappers at
// `src/lib/` are aliased to `tests/e2e/mocks/` (vite.config.ts →
// e2eAliases). pickVideoFiles returns two deliberately-named inputs;
// convertVideoFormat streams Started → FileProgress → Succeeded events
// per file at ~20 ms each and resolves with success_count = 2.
// Failure paths are unit-covered in VideoFormatConverter.test.tsx.

test("converts staged video files and surfaces the done state", async ({
  page,
}) => {
  // Three "Format Converter" tiles exist on the dashboard (image + audio
  // + video); go directly to the video route rather than navigate-by-name.
  await page.goto("/#/tools/video-format-converter");

  // Idle → staging via the picker mock.
  await page.getByRole("button", { name: /^select video files$/i }).click();
  await expect(page.getByRole("button", { name: /^convert$/i })).toBeVisible();
  await expect(page.getByText(/staged \(2\)/i)).toBeVisible();

  // Three target-format radios; MP4 selected by default.
  await expect(page.getByRole("radio", { name: /mp4/i })).toBeChecked();

  // Switch to WebM to exercise the radio + opts.target_format wiring.
  await page.getByRole("radio", { name: /webm/i }).click();
  await expect(page.getByRole("radio", { name: /webm/i })).toBeChecked();

  await page.getByRole("button", { name: /^convert$/i }).click();

  // Done state. Mock returns success_count = 2, skip_count = 0.
  await expect(page.getByText(/2 converted/i)).toBeVisible({ timeout: 5000 });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /convert another/i }),
  ).toBeVisible();
});
