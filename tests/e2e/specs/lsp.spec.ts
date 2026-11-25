import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

// The daemon's fake mode gives rust files the fake language server
// (crates/lsp/src/bin/fake-lsp.rs): one error on every line with TODO.
test("a trusted project's rust file gets diagnostics and hover from its language server", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-lsp-"));
  writeFileSync(join(dir, "main.rs"), "fn main() {\n    // TODO: greet\n}\n");

  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();

  const trust = page.getByTestId("project-trust");
  await trust.getByRole("button", { name: "Review and trust…" }).click();
  await trust.getByTestId("trust-review").getByRole("button", { name: "Trust project" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Project settings: trusted");

  await page.getByRole("treeitem", { name: /main\.rs/ }).click();
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
