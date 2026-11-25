import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

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

test("Mod+Shift+H replaces in files: open files stay unsaved, others are written", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-replace-"));
  writeFileSync(join(dir, "open.txt"), "old\n");
  writeFileSync(join(dir, "closed.txt"), "an old (old)\n");

  await page.goto(daemonUrl());
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /open\.txt/ })).toBeVisible();

  await page.keyboard.press("ControlOrMeta+Shift+F");
  await page.keyboard.type("old");
  const search = page.getByTestId("search");
  await search.getByRole("button", { name: /open\.txt line 1/ }).click();
  await expect(page.getByRole("tab", { name: "open.txt" })).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("ControlOrMeta+Shift+H");
  await expect(search.getByRole("textbox", { name: "Replace with" })).toBeFocused();
  await page.keyboard.type("new");
  await page.keyboard.press("Enter");
  await expect(search.getByRole("alertdialog")).toContainText("Replace 3 matches in 2 files?");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("replace-note")).toHaveText("Replaced 3 matches in 2 files.");

  await expect.poll(() => readFileSync(join(dir, "closed.txt"), "utf8")).toBe("an new (new)\n");
  expect(readFileSync(join(dir, "open.txt"), "utf8")).toBe("old\n");
  await expect(page.getByTestId("main").locator(".cm-content")).toHaveText("new");
  await expect(page.getByRole("tab", { name: "open.txt, unsaved changes" })).toBeVisible();
  await expect(page.getByTestId("search-summary")).toHaveText("No results.");
});
