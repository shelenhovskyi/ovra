import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

test("open files become tabs that switch, show unsaved changes and close", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-tabs-"));
  writeFileSync(join(dir, "a.txt"), "alpha\n");
  writeFileSync(join(dir, "b.txt"), "beta\n");

  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await page.getByRole("treeitem", { name: /a\.txt/ }).click();
  await page.getByRole("treeitem", { name: /b\.txt/ }).click();

  const tabs = page.getByRole("tablist", { name: "Open files" });
  const main = page.getByTestId("main");
  await expect(tabs.getByRole("tab")).toHaveText(["a.txt", "b.txt"]);
  await expect(tabs.getByRole("tab", { name: "b.txt" })).toHaveAttribute("aria-selected", "true");

  await tabs.getByRole("tab", { name: "a.txt" }).click();
  await page.keyboard.press("ControlOrMeta+E");
  await main.locator(".cm-content").click();
  await page.keyboard.type("edited ");
  await expect(tabs.getByRole("button", { name: "Close a.txt (unsaved changes)" })).toBeVisible();

  // Ctrl+Tab keeps the mode; the unsaved text survives the switch.
  await page.keyboard.press("Control+Tab");
  await expect(tabs.getByRole("tab", { name: "b.txt" })).toHaveAttribute("aria-selected", "true");
  await expect(main.locator(".cm-content")).toHaveText("beta");
  await expect(tabs.getByRole("button", { name: "Close a.txt (unsaved changes)" })).toBeVisible();
  await page.keyboard.press("Control+Shift+Tab");
  await expect(main.locator(".cm-content")).toContainText("edited");

  await tabs.getByRole("button", { name: "Close b.txt" }).click();
  await expect(tabs.getByRole("tab")).toHaveText(["a.txt"]);
  await page.keyboard.press("ControlOrMeta+W");
  await expect(tabs).toBeHidden();
});
