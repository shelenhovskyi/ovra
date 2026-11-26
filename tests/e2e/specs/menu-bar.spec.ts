import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { root } from "../daemon";
import { expect, loadProject, test } from "../test";

// The macOS menu bar (app/src/menu.rs) shows each command's shortcut as its
// accelerator. Only the window has it, so this checks its table against the
// commands the UI registers: every item's shortcut is the command's own.
const MENU = readFileSync(join(root, "app/src/menu.rs"), "utf8");
const bound = [...MENU.matchAll(/\brun\("([^"]+)", "[^"]+", "([^"]+)"\)/g)].map(([, id, keys]) => ({ id, keys }));
/** Commands only the desktop window registers: it has the folder dialog. */
const DESKTOP_ONLY = new Set(["tree.open_folder"]);
const unbound = [...MENU.matchAll(/\brun_unbound\("([^"]+)", "[^"]+"\)/g)].map(([, id]) => id);

test("the menu bar shows the shortcuts the commands have", async ({ page }) => {
  expect(bound.length).toBeGreaterThan(10);
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-menu-"));
  writeFileSync(join(dir, "README.md"), "menu\n");
  await loadProject(page, dir, url);
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  await page.keyboard.press("ControlOrMeta+Comma");
  await page.locator("[data-pane=keyboard]").click();
  const table = page.getByRole("table", { name: "Everywhere" });
  await expect(table).toBeVisible();
  const rows = await table
    .locator("tr[data-command]")
    .evaluateAll((trs) =>
      Object.fromEntries(trs.map((tr) => [tr.getAttribute("data-command"), tr.getAttribute("data-keys")])),
    );
  const has = (id: string) => (id in rows ? rows[id] : null);
  const here = bound.filter(({ id }) => !DESKTOP_ONLY.has(id));
  expect(here.map(({ id }) => [id, has(id)])).toEqual(here.map(({ id, keys }) => [id, keys]));
  // An item without an accelerator is a command without a shortcut, or one
  // whose shortcut another item already shows.
  const shown = new Set(bound.map(({ id }) => id));
  expect(unbound.filter((id) => !shown.has(id) && has(id) !== null)).toEqual([]);
});
