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

  // Keyboard: arrows step, Home goes to the minimum; a double click resets.
  await handle.focus();
  await page.keyboard.press("ArrowRight");
  await expect(handle).toHaveAttribute("aria-valuenow", "276");
  await page.keyboard.press("Home");
  await expect(handle).toHaveAttribute("aria-valuenow", "180");
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

  // Swap sides: the rail goes with Files. Then fold Files: no strip, the
  // rail unfolds it. (Agents is not folded here: it unfolds while any
  // project's request waits, and other specs make some.)
  const agents = page.getByTestId("agents");
  const rail = page.getByRole("navigation", { name: "Views" });
  const files = rail.getByRole("button", { name: "Files", exact: true });
  await sidebar.getByRole("button", { name: "Move Files to the other side" }).click();
  expect((await agents.boundingBox())?.x ?? 0).toBeLessThan((await sidebar.boundingBox())?.x ?? 0);
  expect((await rail.boundingBox())?.x ?? 0).toBeGreaterThan((await sidebar.boundingBox())?.x ?? 0);
  await sidebar.getByRole("button", { name: "Collapse Files" }).click();
  await expect(files).toHaveAttribute("aria-pressed", "false");
  await expect(files).toBeFocused();
  expect(Math.round((await sidebar.boundingBox())?.width ?? 0)).toBe(0);

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
  await page.getByRole("textbox", { name: "What should Claude do?" }).focus();
  await page.getByRole("treeitem", { name: /file-079\.txt/ }).focus();
  expect(await page.evaluate(() => document.scrollingElement?.scrollTop)).toBe(0);
  expect((await page.locator("header.title").boundingBox())?.y).toBe(0);
});
