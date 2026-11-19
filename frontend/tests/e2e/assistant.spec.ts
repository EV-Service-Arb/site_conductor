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

test("the composer offers three levels of acting on its own, and a question carries the one picked", async ({ page, viewport }) => {
  test.skip(viewport !== null && viewport.width < 768, "the assistant opens folded on a phone");
  await page.goto("/");
  const assistant = page.locator(ASSISTANT);
  await expect(assistant).toBeVisible();
  await assistant.getByRole("button", { name: /^Before acting: / }).click();
  await expect(page.getByRole("menuitemradio")).toHaveText([/^Ask first/, /^Spend on its own/, /^Act on its own/]);
  await page.getByRole("menuitemradio", { name: /^Spend on its own/ }).click();
  await expect(assistant.getByRole("button", { name: "Before acting: Spend on its own" })).toBeVisible();
  const asked = page.waitForRequest((r) => r.method() === "POST" && r.url().endsWith("/ask"));
  await assistant.locator("textarea").fill("buy a map");
  await assistant.locator("textarea").press("Enter");
  expect((await asked).postDataJSON()).toMatchObject({ text: "buy a map", approval: "spend" });
});
