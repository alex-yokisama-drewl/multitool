import { expect, test } from "@playwright/test";

// Happy-path smoke for the Audio Extractor. Wrappers at `src/lib/` are
// aliased to `tests/e2e/mocks/` (vite.config.ts → e2eAliases).
// pickVideoFile returns the first mock video path; extractAudio streams
// two tracks of Started → FileProgress → Succeeded events at ~20 ms each
// and resolves with track_count = 2. Failure paths are unit-covered in
// AudioExtractor.test.tsx.

test("extracts audio from a video and surfaces the done state", async ({
  page,
}) => {
  // Two tiles named "Audio …" exist (Format Converter + Trimmer under
  // Audio, and the Audio Extractor under Video); go directly to the
  // extractor route rather than navigate-by-name.
  await page.goto("/#/tools/audio-extractor");

  // Idle → picked via the picker mock.
  await page.getByRole("button", { name: /^select video file$/i }).click();
  await expect(
    page.getByRole("button", { name: /^extract audio$/i }),
  ).toBeVisible();

  // Picked view shows the input filename (stem of the first mock video).
  await expect(page.getByText(/holiday\.mov/i)).toBeVisible();

  // Click extract → running → done.
  await page.getByRole("button", { name: /^extract audio$/i }).click();

  // Done state. Mock returns track_count = 2.
  await expect(page.getByText(/extracted 2 tracks/i)).toBeVisible({
    timeout: 5000,
  });
  await expect(
    page.getByRole("button", { name: /open output folder/i }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /extract another/i }),
  ).toBeVisible();
});
