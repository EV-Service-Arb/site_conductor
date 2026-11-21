import { expect, test } from "@playwright/test";

// A trade is one of Google's: picked off its list it is set up as the caller's category there, its window keyed by the gcid.
test("a trade picked off Google's list opens its category, with its head, Publish… and its history", async ({ page }) => {
  await page.goto("/gbps/categories");
  await page.getByRole("button", { name: "A trade to set up" }).click();
  await page.getByPlaceholder("Search trades and listings").fill("chimney sweep");
  await page.getByRole("option", { name: /^Chimney sweep/ }).first().click();
  await expect(page).toHaveURL(/\/gbps\/categories\/gcid(:|%3A)chimney_sweep/);
  await expect(page.getByRole("button", { name: "Publish…" })).toBeVisible();
  await page.getByRole("tab", { name: /History/ }).click();
  await expect(page.getByText("Edited").first()).toBeVisible();
  await page.getByRole("button", { name: "Publish…" }).click();
  await expect(page.getByText(/readable by everyone, for good/)).toBeVisible();
});

test("the library is a page of its own: its filters, then its listings", async ({ page }) => {
  await page.goto("/gbps/library");
  await expect(page.getByPlaceholder("Search titles, abouts and trades")).toBeVisible();
  await expect(page.getByRole("radio", { name: "Standard" }).or(page.getByRole("button", { name: "Standard" }))).toBeVisible();
});
