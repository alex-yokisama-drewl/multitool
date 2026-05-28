import { expect, test } from "@playwright/test";

// Happy-path smoke for the Lorem Ipsum generator. No `src/lib/` wrapper to
// mock — generation is in-component and Copy goes through the webview's own
// `navigator.clipboard.writeText`, stubbed below so the click resolves
// regardless of headless-Chromium's clipboard-permission state.

test("renders paragraphs, regenerates them, and copies to the clipboard", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (text: string) => {
          (window as unknown as { __lastCopy?: string }).__lastCopy = text;
          return Promise.resolve();
        },
      },
    });
  });

  await page.goto("/#/tools/lorem");

  // Initial render: 5 paragraphs in the output region, all non-empty.
  const output = page.getByTestId("lorem-output");
  await expect(output).toBeVisible();
  await expect(output.locator("p")).toHaveCount(5);
  const initial = (await output.textContent()) ?? "";
  expect(initial.length).toBeGreaterThan(0);

  // Regenerate produces a visibly different batch.
  await page.getByRole("button", { name: /^regenerate$/i }).click();
  await expect
    .poll(async () => (await output.textContent()) ?? "")
    .not.toBe(initial);
  await expect(output.locator("p")).toHaveCount(5);

  // Copy transitions the button label through the "Copied" affordance and
  // lands the current text on the (stubbed) clipboard.
  await page.getByRole("button", { name: /^copy$/i }).click();
  await expect(page.getByRole("button", { name: /^copied$/i })).toBeVisible();
  const copied = await page.evaluate(
    () => (window as unknown as { __lastCopy?: string }).__lastCopy,
  );
  expect(copied).toBeTruthy();
  expect(copied!.split("\n\n")).toHaveLength(5);
});
