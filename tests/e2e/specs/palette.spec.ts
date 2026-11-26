import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { expect, loadProject, shortcutLabel, test } from "../test";

function daemonUrl(): string {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return raw;
}

/** Waits for the start screen to load its projects: it may move focus when it does, which would close the palette. */
async function ready(page: Page): Promise<void> {
  await page.goto(daemonUrl());
  await expect(page.getByTestId("status")).toHaveText(/^Connected/);
  await page.waitForFunction(() => {
    const list = document.querySelector("[data-testid=project-list]");
    return list?.querySelector("ul") || list?.textContent?.includes("No projects yet");
  });
}

const palette = (page: Page) => page.getByRole("dialog", { name: "Command palette" });

test("the command palette opens on Mod+Shift+P, filters and closes", async ({ page }) => {
  await ready(page);

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await expect(palette(page)).toBeVisible();
  await expect(page.getByRole("combobox", { name: "Type a command" })).toBeFocused();

  // The palette shows the shortcut that works here: browser-safe ones in a browser tab.
  await page.keyboard.type("go to file");
  await expect(page.getByRole("option", { selected: true })).toContainText(shortcutLabel("Alt+P"));
  await page.getByRole("combobox").fill("");
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

test("Alt+P goes to a file; the palette switches and closes projects", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-palette-"));
  mkdirSync(join(dir, "src/deep/inside"), { recursive: true });
  writeFileSync(join(dir, "src/deep/inside/target.md"), "# Target\n");
  writeFileSync(join(dir, "README.md"), "readme\n");
  // Over 2,000 files, so the list is ranked in the Web Worker.
  mkdirSync(join(dir, "gen"));
  for (let i = 0; i < 2100; i++) writeFileSync(join(dir, "gen", `file-${i}.txt`), "");

  await loadProject(page, dir, daemonUrl());
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  // The title bar's field is "Search or run" on Mod+K: files, and after ">" commands, synonyms too.
  await expect(page.getByTestId("search-field")).toContainText(/Search or run\s*(Ctrl\+K|⌘K)/);
  await page.keyboard.press("ControlOrMeta+K");
  const search = page.getByRole("combobox", { name: "Search files, or type > for commands" });
  await expect(search).toBeFocused();
  await page.keyboard.type("readme");
  await expect(page.getByRole("option", { selected: true })).toContainText("README.md");
  await search.fill(">proxy");
  await expect(page.getByRole("option", { selected: true })).toContainText(/Settings\s*Network\b/);
  await page.keyboard.press("Escape");

  // Headless mode runs in a browser tab, which keeps Ctrl+P for printing.
  await page.keyboard.press("Alt+P");
  await page.keyboard.type("deeptarget");
  await expect(page.getByRole("option", { name: "src/deep/inside/target.md" })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("tab", { name: "target.md" })).toHaveAttribute("aria-selected", "true");

  // Going to a file while editing keeps editing, with the text focused.
  await page.keyboard.press("ControlOrMeta+E");
  const text = page.getByTestId("main").locator(".cm-content");
  await expect(text).toBeFocused();
  await page.keyboard.press("Alt+P");
  await page.keyboard.type("readme");
  await expect(page.getByRole("option", { selected: true })).toContainText("README.md");
  await page.keyboard.press("Enter");
  await expect(text).toHaveText("readme");
  await expect(text).toBeFocused();

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.keyboard.type("close project");
  await page.keyboard.press("Enter");
  const projects = page.getByTestId("project-list");
  await expect(projects).toBeVisible();
  await expect(projects.getByRole("button").first()).toBeFocused();

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
  await ready(page);
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
