import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
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
  await expect(sidebar.getByRole("heading", { name: "Projects" })).toBeVisible();
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
