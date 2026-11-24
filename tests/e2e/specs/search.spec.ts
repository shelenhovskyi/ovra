import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

function daemonUrl(): string {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return raw;
}

test("Mod+Shift+F searches the project's files and opens a result at its match", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-search-"));
  mkdirSync(join(dir, "src"));
  mkdirSync(join(dir, "ignored"));
  writeFileSync(join(dir, ".gitignore"), "ignored/\n");
  writeFileSync(join(dir, "src/main.rs"), "fn main() {\n    let needle = 1;\n}\n");
  writeFileSync(join(dir, "README.md"), "Nothing to see.\n");
  writeFileSync(join(dir, "ignored/skip.txt"), "needle\n");

  await page.goto(daemonUrl());
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  await page.keyboard.press("ControlOrMeta+Shift+F");
  const search = page.getByTestId("search");
  await expect(search.getByRole("searchbox", { name: "Search text" })).toBeFocused();
  await page.keyboard.type("NEEDLE");
  await expect(page.getByTestId("search-summary")).toHaveText("1 matching line in 1 file.");

  await search.getByRole("button", { name: /src\/main\.rs line 2/ }).click();
  await expect(page.getByRole("tab", { name: "main.rs" })).toHaveAttribute("aria-selected", "true");
  const text = page.getByTestId("main").locator(".cm-content");
  await expect(text).toBeFocused();
  await expect.poll(() => page.evaluate(() => document.getSelection()?.toString())).toBe("needle");

  // The tree is one click away and kept its state.
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await expect(search).toBeHidden();
});
