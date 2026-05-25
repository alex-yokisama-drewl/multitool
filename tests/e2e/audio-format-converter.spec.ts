import { expect, test } from "@playwright/test";

// Happy-path smoke for the Audio Format Converter. Wrappers at
// `src/lib/` are aliased to `tests/e2e/mocks/` (vite.config.ts →
// e2eAliases). pickConvertibleAudio returns three deliberately-named
// inputs; convertAudioFormat streams Started → Succeeded events per
// file at 30 ms each and resolves with success_count = 3.
// Failure paths are unit-covered in AudioFormatConverter.test.tsx.

test("converts staged audio files and surfaces the done state", async ({
  page,
}) => {
  // Two "Format Converter" tiles exist on the dashboard (image + audio);
  // go directly to the audio route rather than navigate-by-name.
  await page.goto("/#/tools/audio-format-converter");

  // Idle → staging via the picker mock.
  await page.getByRole("button", { name: /^select audio files$/i }).click();
  await expect(page.getByRole("button", { name: /^convert$/i })).toBeVisible();
  await expect(page.getByText(/staged \(3\)/i)).toBeVisible();

  // Default target = MP3, so the bitrate field should be visible by
  // default. Switch to OGG to exercise the conditional swap.
  await expect(page.getByLabel(/mp3 bitrate/i)).toBeVisible();
  await page.getByRole("radio", { name: "OGG Vorbis" }).click();
  await expect(page.getByLabel(/ogg quality/i)).toBeVisible();
  await expect(page.getByLabel(/mp3 bitrate/i)).not.toBeVisible();

  // Switch back to MP3 for the conversion run (default-path coverage).
  // Use the radio role specifically — "MP3" also appears in the staged
  // filename "track.mp3", which would otherwise make the locator strict-
  // mode-ambiguous.
  await page.getByRole("radio", { name: "MP3" }).click();
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
