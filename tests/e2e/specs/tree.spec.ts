import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

function daemonUrl(): string {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return url;
}

test("the sidebar starts on the project list", async ({ page }) => {
  await page.goto(daemonUrl());
  const sidebar = page.getByTestId("project-list");
  await expect(sidebar.getByRole("heading", { name: "Open a project" })).toBeVisible();
  await expect(sidebar.getByRole("textbox", { name: "Open a folder" })).toBeVisible();
  await expect(sidebar.getByRole("button", { name: "Open", exact: true })).toBeEnabled();
});

test("an added project shows its files and picks up new ones", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-tree-"));
  mkdirSync(join(dir, "src"));
  writeFileSync(join(dir, "src", "lib.rs"), "");
  writeFileSync(join(dir, "README.md"), "");

  await page.goto(daemonUrl());
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();

  const tree = page.getByTestId("file-tree");
  await expect(tree.getByRole("treeitem")).toHaveText([/src/, /README\.md/]);
  await tree.getByRole("treeitem", { name: "src" }).click();
  await expect(tree.getByRole("treeitem", { name: "lib.rs" })).toBeVisible();

  writeFileSync(join(dir, "src", "main.rs"), "");
  await expect(tree.getByRole("treeitem", { name: "main.rs" })).toBeVisible();
});

test("new file from a row's menu, F2 rename in place and drag to move", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-tree-"));
  writeFileSync(join(dir, "notes.md"), "");

  await page.goto(daemonUrl());
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();

  const tree = page.getByTestId("file-tree");
  const notes = tree.getByRole("treeitem", { name: "notes.md" });
  await notes.click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem", { name: /Move to Trash/ })).toBeVisible();
  await menu.getByRole("menuitem", { name: "New file…" }).click();

  // The field keeps focus after the menu gives focus back to the tree.
  const field = tree.getByRole("textbox", { name: "File name" });
  await expect(field).toBeFocused();
  await field.fill("notes.md");
  await expect(tree.getByRole("alert")).toHaveText("“notes.md” exists");
  await field.press("Escape");
  await expect(field).toBeHidden();
  await expect(tree).toBeFocused();

  // A nested name makes its folder; the new file is selected and opens.
  await notes.click({ button: "right" });
  await menu.getByRole("menuitem", { name: "New file…" }).click();
  await field.fill("docs/plan.md");
  await field.press("Enter");
  await expect(tree.getByRole("treeitem", { name: "plan.md" })).toHaveAttribute("aria-selected", "true");
  expect(existsSync(join(dir, "docs", "plan.md"))).toBe(true);

  await notes.click();
  await tree.press("F2");
  const rename = tree.getByRole("textbox", { name: "New name" });
  await expect(rename).toBeFocused();
  expect(await rename.evaluate((el: HTMLInputElement) => [el.selectionStart, el.selectionEnd])).toEqual([0, 5]);
  await rename.press("KeyT");
  await rename.press("Enter");
  const renamed = tree.getByRole("treeitem", { name: "t.md" });
  await expect(renamed).toBeVisible();

  // Dragging a file onto a folder moves it there.
  await renamed.dragTo(tree.getByRole("treeitem", { name: "docs" }));
  await expect(tree.getByRole("treeitem", { name: "t.md" })).toHaveAttribute("aria-level", "2");
  expect(existsSync(join(dir, "docs", "t.md"))).toBe(true);
  expect(existsSync(join(dir, "notes.md"))).toBe(false);
});
