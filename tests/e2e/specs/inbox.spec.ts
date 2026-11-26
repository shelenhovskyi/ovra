import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, loadProject, test } from "../test";

test("reviews open in the main area from the keyboard and give focus back", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-inbox-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await loadProject(page, dir, url);
  const readme = page.getByRole("treeitem", { name: /README\.md/ });
  await expect(readme).toBeVisible();
  await readme.focus();

  // Browser tabs keep Mod+Shift+R (reload), so headless mode uses Alt+Shift+R.
  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByTestId("inbox");
  await expect(inbox).toBeVisible();
  await expect(inbox.getByRole("heading", { name: "Reviews", exact: true })).toBeFocused();
  await expect(page.getByTestId("inbox-button")).toHaveAttribute("aria-pressed", "true");
  // Until the core keeps review items (task 3.B) it says so instead of listing them.
  await expect(inbox).toContainText(/Nothing to review|Could not list reviews/);

  await page.keyboard.press("Escape");
  await expect(inbox).toBeHidden();
  await expect(readme).toBeFocused();
});
