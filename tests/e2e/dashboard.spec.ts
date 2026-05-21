import { test, expect } from "@playwright/test";

// Placeholder spec — skipped until the first tool ships and the dashboard
// has something to assert beyond the empty-state placeholder (which the
// Vitest smoke test already covers).
test.skip("dashboard renders the empty state on a clean registry", async ({
  page,
}) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: /no tools yet/i }),
  ).toBeVisible();
});
