import { expect, test } from "@playwright/test";

const ASSISTANT = '[aria-label="Assistant"]';

test("a new browser finds the assistant open on a wide screen and folded on a phone; folded, it stays folded", async ({ page, viewport }) => {
  await page.goto("/");
  if (viewport !== null && viewport.width < 768) {
    await page.waitForLoadState("networkidle"); // hydrated: what it opens, it has opened
    await expect(page.locator(ASSISTANT)).toHaveCount(0);
    return;
  }
  await expect(page.locator(ASSISTANT)).toBeVisible();
  await expect(page).toHaveURL(/\?assistant=sidebar$/);
  await page.getByRole("button", { name: "Assistant: show or hide" }).click();
  await expect(page.locator(ASSISTANT)).toHaveCount(0);
  await page.goto("/");
  await page.waitForLoadState("networkidle");
  await expect(page.locator(ASSISTANT)).toHaveCount(0);
});
