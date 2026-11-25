import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "../test";

/** Opens a new folder with `count` files and waits for the tree. */
async function openProject(page: Page, prefix: string, count = 1): Promise<void> {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), prefix));
  for (let i = 0; i < count; i++) writeFileSync(join(dir, `file-${String(i).padStart(3, "0")}.txt`), `${i}\n`);
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /file-000\.txt/ })).toBeVisible();
}

test("side panels resize, fold and swap, and the layout survives a reload", async ({ page }) => {
  await openProject(page, "ovra-layout-");
  const sidebar = page.getByTestId("sidebar");
  const handle = page.getByRole("separator", { name: "Resize Files" });
  await expect(handle).toHaveAttribute("aria-valuenow", "260");

  // Keyboard: arrows step, Home and End go to the limits (End leaves the main
  // area 320 px beside Agents in a 1280 px window); a double click resets.
  await handle.focus();
  await page.keyboard.press("ArrowRight");
  await expect(handle).toHaveAttribute("aria-valuenow", "276");
  await page.keyboard.press("End");
  await expect(handle).toHaveAttribute("aria-valuenow", String(1280 - 52 - 16 - 320 - 320));
  await page.keyboard.press("Home");
  await expect(handle).toHaveAttribute("aria-valuenow", "160");
  await handle.dblclick();
  await expect(handle).toHaveAttribute("aria-valuenow", "260");

  // Dragging away from the edge widens it.
  const box = await handle.boundingBox();
  if (!box) throw new Error("no handle box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 60, box.y + box.height / 2);
  await page.mouse.up();
  await expect(handle).toHaveAttribute("aria-valuenow", "316");
  expect(Math.round((await sidebar.boundingBox())?.width ?? 0)).toBe(316);

  // Swap sides from the palette: the rail goes with Files. Then fold Files
  // with the title bar's toggle: no strip, the rail unfolds it. Agents folds
  // on Alt+Mod+0 and comes back the same way, or with New task (Alt+N in a
  // browser tab, Mod+N in the app).
  const agents = page.getByTestId("agents");
  const rail = page.getByRole("navigation", { name: "Views" });
  const files = rail.getByRole("button", { name: "Files", exact: true });
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("combobox", { name: "Type a command" }).fill("Swap side panels");
  await page.keyboard.press("Enter");
  expect((await agents.boundingBox())?.x ?? 0).toBeLessThan((await sidebar.boundingBox())?.x ?? 0);
  expect((await rail.boundingBox())?.x ?? 0).toBeGreaterThan((await sidebar.boundingBox())?.x ?? 0);
  const title = page.locator("header.title");
  await title.getByRole("button", { name: "Sidebar", pressed: true }).click();
  await expect(files).toHaveAttribute("aria-pressed", "false");
  expect(Math.round((await sidebar.boundingBox())?.width ?? 0)).toBe(0);
  const agentsToggle = title.getByRole("button", { name: "Agents", exact: true });
  await page.keyboard.press("Alt+ControlOrMeta+0");
  await expect(agentsToggle).toHaveAttribute("aria-pressed", "false");
  await expect(agents.getByRole("button", { name: /^Show Agents/ })).toBeVisible();
  await page.keyboard.press("Alt+ControlOrMeta+0");
  await expect(agentsToggle).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Alt+ControlOrMeta+0");
  await expect(agentsToggle).toHaveAttribute("aria-pressed", "false");
  await page.keyboard.press("Alt+N");
  await expect(agentsToggle).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("textbox", { name: "Give Claude a task" })).toBeFocused();
  // The rail's bottom: Terminal and Settings, which opens in the main area and closes on a second press.
  const gear = rail.getByRole("button", { name: "Settings" });
  await gear.click();
  const settings = page.getByRole("region", { name: "Settings" });
  await expect(settings.getByRole("heading", { name: "Settings" })).toBeFocused();
  await expect(gear).toHaveAttribute("aria-pressed", "true");
  await gear.click();
  await expect(settings).toBeHidden();

  await page.reload();
  await expect(files).toHaveAttribute("aria-pressed", "false");
  expect((await agents.boundingBox())?.x ?? 0).toBeLessThan((await rail.boundingBox())?.x ?? 0);
  // The rail's Files button unfolds it, at the width it had.
  await files.click();
  await expect(page.getByRole("separator", { name: "Resize Files" })).toHaveAttribute("aria-valuenow", "316");
});

test("the window never scrolls, whatever a panel scrolls into view", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 600 });
  await openProject(page, "ovra-noscroll-", 80);
  // The tree only renders rows near its viewport: End scrolls the last one in.
  await page.getByRole("treeitem", { name: /file-000\.txt/ }).click();
  await page.keyboard.press("End");
  await page.getByRole("treeitem", { name: /file-079\.txt/ }).click();
  await page.getByRole("textbox", { name: "Give Claude a task" }).focus();
  await page.getByRole("treeitem", { name: /file-079\.txt/ }).focus();
  expect(await page.evaluate(() => document.scrollingElement?.scrollTop)).toBe(0);
  expect((await page.locator("header.title").boundingBox())?.y).toBe(0);
});
