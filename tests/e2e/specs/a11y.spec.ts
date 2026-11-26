import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, expectAccessible, loadProject, test } from "../test";

// axe on the screens every session passes through, in each look a person can
// ask the system for. The agent, review, git and Focus flows run it in their
// own tests (agents.spec.ts, git.spec.ts), where those screens are reached.

test("the start screen passes axe in dark and in light with increased contrast", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto(process.env.OVRA_URL ?? "");
  await expect(page.getByRole("heading", { name: "Open a project" })).toBeVisible();
  await expect(page.getByTestId("claude-status")).toContainText("is ready");
  // The project list is the page's main content until a project opens.
  await expect(page.getByRole("main")).toContainText("Open a project");
  await expectAccessible(page, "on the start screen");
  await page.emulateMedia({ colorScheme: "light", contrast: "more" });
  await expectAccessible(page, "on the start screen, light, increased contrast");
});

test("an open project with a file and the palette passes axe in light, dark and increased contrast", async ({
  page,
}) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-a11y-"));
  writeFileSync(join(dir, "README.md"), "# Notes\n\nSome text.\n");
  writeFileSync(join(dir, "main.rs"), 'fn main() {\n    println!("hi");\n}\n');
  await page.emulateMedia({ colorScheme: "light" });
  await loadProject(page, dir);
  await page.getByRole("treeitem", { name: /main\.rs/ }).dblclick();
  await expect(page.getByRole("tab", { name: "main.rs" })).toBeVisible();
  await expect(page.getByTestId("main")).toContainText("println");
  // The tab list holds only tabs; the pointer's pin and close buttons are not read out.
  await expect(page.getByRole("tablist", { name: "Open files" }).getByRole("button")).toHaveCount(0);
  await expectAccessible(page, "with a file open, light");
  await page.emulateMedia({ colorScheme: "dark", contrast: "more" });
  await expectAccessible(page, "with a file open, dark, increased contrast");
  await page.keyboard.press("Alt+P");
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  await expectAccessible(page, "with the palette open");
});

test("in forced colors every focus ring is still drawn", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-a11y-forced-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await page.emulateMedia({ forcedColors: "active" });
  await loadProject(page, dir);
  // Forced colors drop box-shadow and accent fills: the splitter and the task
  // box, which ring that way, show an outline instead.
  const ring = (selector: string) =>
    page.evaluate((s) => {
      const style = getComputedStyle(document.querySelector(s) as Element);
      return style.outlineStyle !== "none" && Number.parseFloat(style.outlineWidth) >= 2;
    }, selector);
  await page.getByRole("separator", { name: "Resize Agents" }).focus();
  expect(await ring(":focus-visible")).toBe(true);
  await page.getByRole("textbox", { name: "Describe a task" }).press("a");
  expect(await ring("[data-testid=agents] .compose:has(textarea:focus-visible)")).toBe(true);
});
