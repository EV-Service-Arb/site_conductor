import { expect, test } from "@playwright/test";

test.skip(({ viewport }) => viewport !== null && viewport.width < 768, "a phone's windows are pages");

test("a window minimized comes back from the hotbar with what was typed in it, over another window and another page; closed, it does not", async ({ page }) => {
  await page.goto("/?library=");
  const library = page.getByRole("dialog", { name: "Library" });
  const search = library.getByPlaceholder("Search titles, abouts and trades");
  await search.fill("half a thought");
  await library.getByRole("button", { name: "To the hotbar" }).click();
  await expect(library).toBeHidden();
  const hotbar = page.getByRole("region", { name: "Hotbar" });
  await page.getByRole("navigation", { name: "Outline" }).getByRole("button", { name: "Trash" }).click();
  await expect(page.getByRole("dialog", { name: "Trash" })).toBeVisible();
  await page.getByRole("link", { name: "Alerts" }).click();
  await expect(page).toHaveURL(/\/alerts/);
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const entry = hotbar.locator('[data-slot="hotbar-action"]', { hasText: "Library" });
  await entry.click();
  await expect(search).toHaveValue("half a thought");
  await library.getByRole("button", { name: "Close the window" }).click();
  await expect(entry).toHaveCount(0);
  await page.goto("/?library=");
  await expect(search).toHaveValue("");
});

test("a panel popped out of the hotbar minimizes to its action, which brings it back where it was", async ({ page }) => {
  await page.goto("/");
  const action = page.getByRole("region", { name: "Hotbar" }).locator('[data-slot="hotbar-action"]', { hasText: "Review kits" });
  await action.click();
  await page.getByRole("button", { name: "Pop out" }).click();
  const panel = page.getByRole("dialog", { name: "Review kits" });
  const bar = (await panel.locator('[data-slot="floating-panel-title"]').boundingBox())!;
  await page.mouse.move(bar.x + 20, bar.y + 10);
  await page.mouse.down();
  await page.mouse.move(bar.x + 120, bar.y - 90);
  await page.mouse.up();
  const left = await panel.boundingBox();
  await panel.getByRole("button", { name: "Minimize" }).click();
  await expect(panel).toBeHidden();
  await action.click();
  await expect(panel).toBeVisible();
  expect(await panel.boundingBox()).toEqual(left);
});
