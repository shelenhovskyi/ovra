import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

test("edit a file with highlighting and save it to disk", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-editor-"));
  writeFileSync(join(dir, "main.rs"), "fn main() {}\n");

  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Project folder" }).fill(dir);
  await projects.getByRole("button", { name: "Add" }).click();
  await page.getByRole("treeitem", { name: /main\.rs/ }).click();
  await page.keyboard.press("ControlOrMeta+E");

  const editor = page.getByTestId("main");
  await expect(editor.locator(".cm-content")).toHaveText("fn main() {}");
  await expect(editor.locator(".tok-keyword").first()).toHaveText("fn");

  await editor.locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("// saved");
  await expect(page.getByTestId("editor-dirty")).toBeVisible();
  await page.keyboard.press("ControlOrMeta+S");
  await expect(page.getByTestId("editor-dirty")).toBeHidden();
  expect(readFileSync(join(dir, "main.rs"), "utf8")).toBe("fn main() {}\n// saved");
});
