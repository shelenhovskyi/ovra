import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

test("the review inbox opens from the keyboard and gives focus back", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-inbox-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  const readme = page.getByRole("treeitem", { name: /README\.md/ });
  await expect(readme).toBeVisible();
  await readme.focus();

  // Browser tabs keep Mod+Shift+R (reload), so headless mode uses Alt+Shift+R.
  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByRole("dialog", { name: "Review inbox" });
  await expect(inbox).toBeVisible();
  await expect(inbox.getByRole("heading", { name: "Review inbox" })).toBeFocused();
  // Until the core keeps review items (task 3.B) it says so instead of listing them.
  await expect(inbox).toContainText(/No agent changes wait for review\.|Could not list reviews/);

  await page.keyboard.press("Escape");
  await expect(inbox).toBeHidden();
  await expect(readme).toBeFocused();
});
