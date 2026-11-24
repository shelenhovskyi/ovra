import { expect, test } from "@playwright/test";

function daemonUrl(): string {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return raw;
}

test("the command palette opens on Mod+Shift+P, filters and closes", async ({ page }) => {
  await page.goto(daemonUrl());
  await expect(page.getByTestId("status")).toHaveText(/^Connected/);

  const palette = page.getByRole("dialog", { name: "Command palette" });
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await expect(palette).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Command" })).toBeFocused();

  await page.keyboard.type("shpal");
  await expect(page.getByRole("option", { name: /Show command palette/ })).toHaveAttribute("aria-selected", "true");

  await page.keyboard.type("zzz");
  await expect(palette).toContainText("No matching commands");

  await page.keyboard.press("Escape");
  await expect(palette).toBeHidden();

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await expect(palette).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(palette).toBeHidden();
});
