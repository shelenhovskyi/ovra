import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { expect, loadProject, test } from "../test";

test("edit a file with highlighting and save it to disk", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-editor-"));
  writeFileSync(join(dir, "main.rs"), "fn main() {\n    run();\n}\n");

  await loadProject(page, dir, url);
  await page.getByRole("treeitem", { name: /main\.rs/ }).click();
  await page.keyboard.press("ControlOrMeta+E");

  const editor = page.getByTestId("main");
  await expect(editor.locator(".cm-line").first()).toHaveText("fn main() {");
  await expect(editor.locator(".tok-keyword").first()).toHaveText("fn");

  await editor.locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("// saved");
  await expect(page.getByTestId("editor-dirty")).toBeVisible();
  await page.keyboard.press("ControlOrMeta+S");
  await expect(page.getByTestId("editor-dirty")).toBeHidden();
  expect(readFileSync(join(dir, "main.rs"), "utf8")).toBe("fn main() {\n    run();\n}\n// saved");

  // The breadcrumbs name the function around the cursor; its body folds and unfolds.
  await page.keyboard.press("ControlOrMeta+Home");
  await expect(page.getByRole("navigation", { name: "Breadcrumbs" }).locator(".symbol")).toHaveText("main");
  await page.keyboard.press("Control+Shift+BracketLeft");
  await expect(editor.locator(".cm-foldPlaceholder")).toBeVisible();
  await page.keyboard.press("Control+Shift+BracketRight");
  await expect(editor.locator(".cm-foldPlaceholder")).toBeHidden();
});

test("reopens the file that was open when the project was left", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-reopen-"));
  writeFileSync(join(dir, "notes.md"), "# Notes\n");

  // Resolves once the core has stored notes.md as the active file.
  const saved = new Promise<void>((resolve) => {
    page.on("websocket", (ws) => {
      let id: number | null = null;
      ws.on("framesent", ({ payload }) => {
        const frame = JSON.parse(String(payload));
        if (frame.method === "projects.set_state" && frame.params.state.active_file === "notes.md") id = frame.id;
      });
      ws.on("framereceived", ({ payload }) => {
        if (id !== null && JSON.parse(String(payload)).id === id) resolve();
      });
    });
  });

  await loadProject(page, dir, url);
  await page.getByRole("treeitem", { name: /notes\.md/ }).click();
  await expect(page.getByTestId("main")).toContainText("Notes");
  await saved;

  // A reload opens the project again by itself, with its file.
  await page.reload();
  await expect(page.getByTestId("project-name")).toContainText(basename(dir));
  await expect(page.getByTestId("main")).toContainText("Notes");
});

test("the editor's context menu finds in the file, and Ctrl+G goes to a line", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-menu-"));
  writeFileSync(join(dir, "main.rs"), "fn main() {}\n");

  await loadProject(page, dir, url);
  await page.getByRole("treeitem", { name: /main\.rs/ }).click();
  await page.keyboard.press("ControlOrMeta+E");
  const content = page.getByTestId("main").locator(".cm-content");
  await expect(content).toHaveText("fn main() {}");

  await content.click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem", { name: "Copy relative path", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(content).toBeFocused();

  await content.click({ button: "right" });
  await menu.getByRole("menuitem", { name: "Find", exact: true }).click();
  await expect(page.locator(".cm-search input[name=search]")).toBeFocused();

  await content.click();
  await page.keyboard.press("Control+G");
  await expect(page.locator(".cm-goto-line input")).toBeFocused();

  // The status bar's cursor follows the selection and opens "Go to line…" too.
  await page.keyboard.press("Escape");
  await content.click();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.press("Shift+Home");
  const facts = page.getByTestId("file-status");
  // No indentation in the text: the tab size preference (4 by default) says.
  await expect(facts).toHaveText(/Ln 2, Col 1\s+Rust\s+Spaces: 4\s+LF/);
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Shift+End");
  await expect(facts.getByRole("button")).toHaveText("Ln 1, Col 13 (12 selected)");
  await facts.getByRole("button").click();
  await expect(page.locator(".cm-goto-line input")).toBeFocused();
});
