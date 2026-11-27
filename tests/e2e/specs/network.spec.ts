import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, loadProject, test } from "../test";

test("a project's allowed hosts are added in Settings and kept, and keys sit next to them", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-network-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await loadProject(page, dir, url);
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  // Mod+, opens Settings; the pane list moves with the arrow keys.
  await page.keyboard.press("ControlOrMeta+Comma");
  const settings = page.getByRole("region", { name: "Settings" });
  await expect(settings.getByRole("heading", { name: "Settings" })).toBeFocused();
  await settings.getByRole("navigation", { name: "Settings panes" }).getByRole("button", { name: "Rules" }).click();
  await page.keyboard.press("ArrowDown");
  const dialog = settings.getByRole("region", { name: "Network" });
  await dialog.getByTestId("add-host").fill("registry.npmjs.org");
  await dialog.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(dialog.getByTestId("allowed-host")).toHaveText(/registry\.npmjs\.org/);
  // Keys sit next: a preset asks only for the value. Whether a keychain
  // exists depends on the machine; either way the pane says which.
  await page.keyboard.press("ArrowDown");
  const keys = settings.getByRole("region", { name: "Keys" });
  await keys.getByRole("button", { name: "GitHub" }).click();
  const available = keys.getByTestId("store-available");
  await expect(available.or(keys.getByTestId("store-unavailable"))).toBeVisible();
  if (await available.isVisible()) {
    // The core named the keychain: the form is ready, not still checking.
    await expect(available).toContainText("Kept in");
    await expect(keys.getByTestId("key-value")).toBeEnabled();
  } else {
    await expect(keys.getByTestId("store-retry")).toBeVisible();
    await expect(keys.getByTestId("key-value")).toHaveCount(0);
  }

  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();
  // A new page reads the hosts from the core again; the project opens by itself.
  await page.reload();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+Comma");
  await settings.getByRole("button", { name: "Network" }).click();
  await expect(page.getByTestId("allowed-host")).toHaveText(/registry\.npmjs\.org/);
});
