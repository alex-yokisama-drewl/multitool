import { expect, test } from "@playwright/test";

// Happy-path smoke for the Audio Trimmer. Wrappers at `src/lib/` are
// aliased to `tests/e2e/mocks/` (vite.config.ts → e2eAliases):
// pickAudioFile returns the first canned audio path; loadAudioPreview
// returns a synthetic peaks array; trimAudio resolves after 30 ms with
// `{stem}_trimmed.{ext}` as the output. Failure paths are unit-covered
// in AudioTrimmer.test.tsx.

test("trims a picked audio file and surfaces the done state", async ({
  page,
}) => {
  // Two "Format Converter"-like cousins now live under Audio; navigate
  // straight to the trimmer route rather than click-by-name.
  await page.goto("/#/tools/audio-trimmer");

  // Idle → picked via the picker mock + loadAudioPreview mock.
  await page.getByRole("button", { name: /^select audio file$/i }).click();
  await expect(page.getByRole("button", { name: /^trim$/i })).toBeVisible();

  // Waveform canvas + numeric inputs are rendered at the mocked
  // duration (5_000 ms).
  await expect(page.getByLabel(/^audio waveform$/i)).toBeVisible();
  await expect(page.getByLabel(/^start$/i)).toHaveValue("00:00.000");
  await expect(page.getByLabel(/^end$/i)).toHaveValue("00:05.000");

  // Toggle a fade to exercise the checkbox wiring before running.
  await page.getByLabel(/fade in/i).check();
  await expect(page.getByLabel(/fade in/i)).toBeChecked();

  // Trim → done.
  await page.getByRole("button", { name: /^trim$/i }).click();
  await expect(page.getByText(/song_trimmed\.wav/i)).toBeVisible({
    timeout: 5000,
  });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /trim another/i }),
  ).toBeVisible();
});
