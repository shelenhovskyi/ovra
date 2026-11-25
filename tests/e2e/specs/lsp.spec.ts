import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, type Page, test } from "@playwright/test";

/** Opens a fresh project holding `files`, trusts it and shows `main.rs` in the viewer. */
async function trustedProject(page: Page, files: Record<string, string>): Promise<void> {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-lsp-"));
  for (const [name, text] of Object.entries(files)) writeFileSync(join(dir, name), text);
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  const trust = page.getByTestId("project-trust");
  await trust.getByRole("button", { name: "Review and trust…" }).click();
  await expect(trust.getByTestId("trust-language-servers")).toContainText("language servers");
  await trust.getByTestId("trust-review").getByRole("button", { name: "Trust project" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Project settings: trusted");
  await page.getByRole("treeitem", { name: /main\.rs/ }).click();
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

  // A hover only asks once the pointer moves onto the word; until the
  // server has answered `initialize` it is not asked at all, so move again.
  const word = editor.locator(".cm-content").getByText("main").first();
  await expect(async () => {
    await page.mouse.move(0, 0);
    await word.hover();
    await expect(page.locator(".cm-tooltip-hover")).toContainText("fake hover", { timeout: 1000 });
  }).toPass({ timeout: 8000 });

  // The footer names the running server; stopping it clears the diagnostic.
  const item = page.getByRole("button", { name: /fake-lsp/ });
  await item.click();
  await page.getByRole("button", { name: "Stop fake-lsp" }).click();
  await expect(editor.locator(".cm-lintRange-error")).toHaveCount(0);
});

test("references reach files that are not open; a rename that would change them is refused", async ({ page }) => {
  await trustedProject(page, {
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
  await expect(page.getByRole("alert")).toContainText("Rename not applied: it would also change other.rs");
  await expect(editor.locator(".cm-content")).toContainText("fn greet()");
  // Servers are capped per daemon, which every test shares.
  await page.getByRole("button", { name: /fake-lsp/ }).click();
  await page.getByRole("button", { name: "Stop fake-lsp" }).click();
});
