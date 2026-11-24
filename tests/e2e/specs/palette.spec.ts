import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, type Page, test } from "@playwright/test";

function daemonUrl(): string {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return raw;
}

const palette = (page: Page) => page.getByRole("dialog", { name: "Command palette" });

test("the command palette opens on Mod+Shift+P, filters and closes", async ({ page }) => {
  await page.goto(daemonUrl());
  await expect(page.getByTestId("status")).toHaveText(/^Connected/);

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await expect(palette(page)).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Type a command" })).toBeFocused();

  await page.keyboard.type("swpro");
  await expect(page.getByRole("option", { name: /Switch project/ })).toHaveAttribute("aria-selected", "true");

  await page.keyboard.type("zzz");
  await expect(palette(page)).toContainText("No matching commands");

  await page.keyboard.press("Escape");
  await expect(palette(page)).toBeHidden();

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await expect(palette(page)).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(palette(page)).toBeHidden();
});

test("Mod+P goes to a file; the palette switches and closes projects", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-palette-"));
  mkdirSync(join(dir, "src/deep/inside"), { recursive: true });
  writeFileSync(join(dir, "src/deep/inside/target.md"), "# Target\n");
  writeFileSync(join(dir, "README.md"), "readme\n");

  await page.goto(daemonUrl());
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  await page.keyboard.press("ControlOrMeta+P");
  await page.keyboard.type("deeptarget");
  await expect(page.getByRole("option", { name: "src/deep/inside/target.md" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("tab", { name: "target.md" })).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.keyboard.type("close project");
  await page.keyboard.press("Enter");
  await expect(projects).toBeVisible();

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.keyboard.type("switch project");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("combobox", { name: "Switch to project" })).toBeFocused();
  const name = dir.split("/").pop() ?? "";
  await page.keyboard.type(name);
  await expect(page.getByRole("option", { selected: true })).toContainText(name);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
});

test("forced colours keep the selected option visible", async ({ page }) => {
  await page.emulateMedia({ forcedColors: "active" });
  await page.goto(daemonUrl());
  await expect(page.getByTestId("status")).toHaveText(/^Connected/);
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const option = page.locator("[role=option][aria-selected=true]");
  await expect(option).toBeVisible();
  const style = await option.evaluate((el) => {
    const s = getComputedStyle(el);
    return { outline: s.outlineStyle, background: s.backgroundColor, color: s.color };
  });
  expect(style.outline).toBe("solid");
  expect(style.background).not.toBe(style.color);
});
