import { expect, test } from "@playwright/test";

test.skip(({ viewport }) => viewport !== null && viewport.width < 768, "a phone has no keyboard to hold the leader on");

// The assistant's composer takes focus as it mounts and `?assistant=sidebar` keeps it open across loads:
// the leader dead inside a field left the whole keymap dead, its own toggle included.
test("the leader reaches the page from the assistant's focused composer, with Alt", async ({ page }) => {
  await page.goto("/?assistant=sidebar");
  const composer = page.locator("textarea:focus");
  await expect(composer).toBeVisible();
  await page.keyboard.press("z");
  await expect(composer).toHaveValue("z");
  await page.keyboard.press("Alt+z");
  await page.keyboard.press("g");
  await page.keyboard.press("e");
  await expect(page).toHaveURL(/\/alerts\/?\?assistant=sidebar$/);
});

test("a key the open scope does not bind reaches the field, and Esc closes the scope without reaching it", async ({ page }) => {
  await page.goto("/?assistant=sidebar");
  const composer = page.locator("textarea:focus");
  await expect(composer).toBeVisible();
  await page.keyboard.press("Alt+z");
  await page.keyboard.press("Escape");
  await page.keyboard.type("hi");
  await expect(composer).toHaveValue("hi");
  await page.keyboard.press("Alt+z");
  await page.keyboard.press("j");
  await expect(composer).toHaveValue("hij");
});

test("outside a field the leader is z, with Alt or without", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Assistant: show or hide", pressed: true }).click(); // a new browser opens it, its composer focused
  for (const leader of ["z", "Alt+z"]) {
    // Pressed again until the keymap listens: it attaches once the page hydrates. The leader pressed inside its own scope stays in it.
    await expect(async () => {
      await page.keyboard.press(leader);
      await expect(page.getByText("Keybinds")).toBeVisible({ timeout: 500 });
    }).toPass();
    await page.keyboard.press("Escape");
    await expect(page.getByText("Keybinds")).toBeHidden();
  }
});

test("z T floats the trash over the page, and Escape within it closes it", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Assistant: show or hide", pressed: true }).click(); // a new browser opens it, its composer focused
  const trash = page.getByRole("dialog", { name: "Trash" });
  await expect(async () => {
    await page.keyboard.press("z");
    await page.keyboard.press("Shift+T");
    await expect(trash).toBeVisible({ timeout: 500 });
  }).toPass();
  await page.keyboard.press("Escape");
  await expect(trash).toBeHidden();
  await expect(page).not.toHaveURL(/trash/);
});
