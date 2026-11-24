import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { expect, test } from "@playwright/test";

test("edit a file with highlighting and save it to disk", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-editor-"));
  writeFileSync(join(dir, "main.rs"), "fn main() {}\n");

  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
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

  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await page.getByRole("treeitem", { name: /notes\.md/ }).click();
  await expect(page.getByTestId("main")).toContainText("Notes");
  await saved;

  await page.reload();
  await page
    .getByTestId("project-list")
    .getByRole("button", { name: basename(dir) })
    .click();
  await expect(page.getByTestId("main")).toContainText("Notes");
});
