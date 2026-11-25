import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

test("a project's allowed hosts are added in the network dialog and kept", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-network-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  await page.getByTestId("more").click();
  await page.getByRole("menuitem", { name: "Network" }).click();
  const dialog = page.getByRole("dialog", { name: "Network and secrets" });
  await expect(dialog.getByRole("heading", { name: "Network and secrets" })).toBeFocused();
  await dialog.getByTestId("add-host").fill("registry.npmjs.org");
  await dialog.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(dialog.getByTestId("allowed-host")).toHaveText(/registry\.npmjs\.org/);
  // Whether a keychain exists depends on the machine; either way the dialog says so.
  await expect(dialog.getByTestId("store-available").or(dialog.getByTestId("store-unavailable"))).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  // A new page reads the hosts from the core again.
  await page.reload();
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await page.getByTestId("more").click();
  await page.getByRole("menuitem", { name: "Network" }).click();
  await expect(page.getByTestId("allowed-host")).toHaveText(/registry\.npmjs\.org/);
});
