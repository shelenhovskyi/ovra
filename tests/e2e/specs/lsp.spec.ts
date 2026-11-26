import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { clickWhenArmed, expect, test } from "../test";

/** Opens a fresh project holding `files`, trusts it and shows `main.rs` in the viewer. */
async function trustedProject(page: Page, files: Record<string, string>): Promise<string> {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-lsp-"));
  for (const [name, text] of Object.entries(files)) writeFileSync(join(dir, name), text);
  // Trust buttons wait a moment after the card changes; the clock skips that.
  await page.clock.install();
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  // No .claude/ here: no trust card until the status bar's item asks for it.
  const trust = page.getByTestId("project-trust");
  await page.getByRole("treeitem", { name: /main\.rs/ }).click();
  await expect(trust).toHaveCount(0);
  // The editor asks for a server, so the status bar offers to turn them on.
  await page.keyboard.press("ControlOrMeta+E");
  await page.getByTestId("lsp-status").click();
  await page.getByTestId("lsp-panel").getByRole("button", { name: "Review and trust…" }).click();
  await expect(trust.getByTestId("trust-language-servers")).toContainText("language servers");
  await clickWhenArmed(page, trust.getByRole("button", { name: "Trust project" }));
  await expect(trust).toHaveCount(0);
  // The server starts; then back to the viewer, where each test starts.
  await expect(page.getByRole("button", { name: /fake-lsp/ })).toBeVisible();
  await page.getByRole("treeitem", { name: /main\.rs/ }).click();
  await page.keyboard.press("ControlOrMeta+E");
  await expect(page.getByTestId("lsp-edit-hint")).toBeVisible();
  return dir;
}

// The daemon's fake mode gives rust files the fake language server
// (crates/lsp/src/bin/fake-lsp.rs): one error on every line with TODO.
test("a trusted project's rust file gets diagnostics and hover from its language server", async ({ page }) => {
  await trustedProject(page, { "main.rs": "fn main() {\n    // TODO: greet\n}\n" });
  // The viewer has no language features and says where they are.
  await expect(page.getByTestId("lsp-edit-hint")).toHaveText("fake-lsp in Edit");
  await page.keyboard.press("ControlOrMeta+E");
  const editor = page.getByTestId("main");
  await expect(editor.locator(".cm-lintRange-error")).toHaveCount(1);

  // The status bar counts it, and the Problems list opens it at its spot.
  await page.getByTestId("problem-counts").filter({ hasText: "1 error" }).click();
  const problem = page.getByTestId("problems").getByRole("option", { name: /fake: TODO found/ });
  await expect(problem).toContainText("2:8");
  await expect(problem).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(editor.locator(".cm-content")).toBeFocused();

  // Quick fix at the problem: the fake offers to replace the TODO, which clears it (unsaved).
  await page.keyboard.press("ControlOrMeta+Period");
  await page.getByRole("menuitem", { name: "Replace TODO with DONE" }).click();
  await expect(editor.locator(".cm-content")).toContainText("// DONE: greet");
  await expect(editor.locator(".cm-lintRange-error")).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+Z");
  await expect(editor.locator(".cm-lintRange-error")).toHaveCount(1);

  // A hover only asks once the pointer moves onto the word; until the
  // server has answered `initialize` it is not asked at all, so move again.
  const word = editor.locator(".cm-content").getByText("main").first();
  await expect(async () => {
    await page.mouse.move(0, 0);
    await word.hover();
    await expect(page.locator(".cm-tooltip-hover")).toContainText("fake hover", { timeout: 1000 });
  }).toPass({ timeout: 8000 });

  // Search or run opens from an editor with a server too: hover's key is Mod+I, not a Mod+K chord.
  await expect(editor.locator(".cm-content")).toBeFocused();
  await page.keyboard.press("ControlOrMeta+K");
  await expect(page.getByRole("combobox", { name: "Search files, or type > for commands" })).toBeFocused();
  await page.keyboard.press("Escape");

  // The footer names the running server; stopping it clears the diagnostic.
  const item = page.getByRole("button", { name: /fake-lsp/ });
  await item.click();
  await page.getByRole("button", { name: "Stop fake-lsp" }).click();
  await expect(editor.locator(".cm-lintRange-error")).toHaveCount(0);
  await expect(page.getByTestId("problem-counts")).toHaveCount(0);
  await expect(page.getByTestId("problems")).toContainText("No problems.");
});

test("references and rename reach files that are not open; renamed files wait unsaved", async ({ page }) => {
  const dir = await trustedProject(page, {
    "main.rs": "fn greet() {}\nfn main() { greet(); }\n",
    "other.rs": "use crate::greet;\n",
  });
  await page.keyboard.press("ControlOrMeta+E");
  const editor = page.getByTestId("main");
  // The cursor goes on the word only once the editor shows the file.
  await expect(page.getByRole("button", { name: /fake-lsp/ })).toBeVisible();
  // Into `greet` on the first line.
  const onGreet = async () => {
    await editor.locator(".cm-content").focus();
    await page.keyboard.press("ControlOrMeta+Home");
    for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowRight");
  };
  await onGreet();
  // Keys work once the server answered `initialize`.
  const panel = page.locator(".cm-lsp-reference-panel");
  await expect(async () => {
    await page.keyboard.press("Shift+F12");
    await expect(panel).toContainText("use crate::greet;", { timeout: 1000 });
  }).toPass({ timeout: 8000 });
  await expect(panel.locator("[role=option]")).toHaveCount(3);
  await page.keyboard.press("Escape");

  await onGreet();
  await page.keyboard.press("F2");
  const name = page.locator(".cm-lsp-rename-panel input");
  await name.fill("hello");
  await name.press("Enter");
  await expect(page.getByRole("alert")).toContainText("none saved yet: this file, other.rs");
  await expect(editor.locator(".cm-content")).toContainText("fn hello()");
  // other.rs got a tab with the edit, unsaved; nothing was written yet.
  await page.getByRole("tab", { name: /other\.rs/ }).click();
  await expect(editor.locator(".cm-content")).toContainText("use crate::hello;");
  expect(readFileSync(join(dir, "other.rs"), "utf8")).toBe("use crate::greet;\n");
  // Servers are capped per daemon, which every test shares.
  await page.getByRole("button", { name: /fake-lsp/ }).click();
  await page.getByRole("button", { name: "Stop fake-lsp" }).click();
});
