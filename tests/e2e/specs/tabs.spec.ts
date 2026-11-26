import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import type { Page } from "@playwright/test";
import { expect, loadProject, test } from "../test";

function project(files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "ovra-tabs-"));
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, name)), { recursive: true });
    writeFileSync(join(dir, name), text);
  }
  return dir;
}

async function open(page: Page, dir: string): Promise<void> {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  await loadProject(page, dir, url);
}

test("tabs switch with the keyboard, keep undo and focus, and ask before closing unsaved text", async ({ page }) => {
  const dir = project({ "a.txt": "alpha\n", "b.txt": "beta\n" });
  await open(page, dir);
  await page.getByRole("treeitem", { name: /a\.txt/ }).dblclick();
  await page.getByRole("treeitem", { name: /b\.txt/ }).click();

  const tabs = page.getByRole("tablist", { name: "Open files" });
  const content = page.getByTestId("main").locator(".cm-content");
  await expect(tabs.getByRole("tab")).toHaveText(["a.txt", "b.txt"]);

  // Ctrl+E puts the caret in the text right away.
  await tabs.getByRole("tab", { name: "a.txt" }).click();
  await page.keyboard.press("ControlOrMeta+E");
  await expect(content).toBeFocused();
  await page.keyboard.type("edited ");
  await expect(tabs.getByRole("tab", { name: "a.txt, unsaved changes" })).toBeVisible();

  // Arrows move between tabs, each in its own mode; the edits and their undo history stay.
  await tabs.getByRole("tab", { name: /a\.txt/ }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.getByRole("tab", { name: "b.txt" })).toBeFocused();
  await expect(page.getByTestId("text-view")).toHaveText("beta");
  await page.keyboard.press("Home");
  await expect(content).toContainText("edited alpha");
  await content.click();
  await page.keyboard.press("ControlOrMeta+Z");
  await expect(content).toHaveText("alpha");
  await page.keyboard.press("ControlOrMeta+Shift+Z");
  await expect(content).toContainText("edited");

  // Closing asks in a modal: Tab stays inside, Cancel keeps everything, Save writes and closes.
  await tabs.getByRole("button", { name: "Close a.txt (unsaved changes)", includeHidden: true }).click();
  const question = page.getByRole("dialog", { name: "Save changes to a.txt before closing?" });
  await expect(question.getByRole("button", { name: "Save", exact: true })).toBeFocused();
  for (let i = 0; i < 4; i++) await page.keyboard.press("Tab");
  await expect(question.getByRole("button", { name: "Don't save" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(question).toBeHidden();
  await expect(tabs.getByRole("tab")).toHaveCount(2);

  await tabs.getByRole("button", { name: "Close a.txt (unsaved changes)", includeHidden: true }).click();
  await question.getByRole("button", { name: "Save", exact: true }).click();
  await expect(tabs.getByRole("tab")).toHaveText(["b.txt"]);
  await expect.poll(() => readFileSync(join(dir, "a.txt"), "utf8")).toBe("edited alpha\n");
  await expect(page.getByTestId("text-view")).toHaveText("beta");

  // Cancel goes back to the text; Ctrl+S while the question is open saves and closes it.
  await page.keyboard.press("ControlOrMeta+E");
  await expect(content).toBeFocused();
  await page.keyboard.type("more ");
  await page.keyboard.press("Alt+W");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(content).toBeFocused();
  await page.keyboard.press("Alt+W");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("ControlOrMeta+S");
  await expect(page.getByRole("dialog")).toBeHidden();
  // The question closes as the save starts; the file is written a moment later.
  await expect.poll(() => readFileSync(join(dir, "b.txt"), "utf8")).toContain("more ");

  // With the last tab gone, focus goes to the file tree, not the page body.
  await expect(tabs).toBeHidden();
  await expect(page.getByRole("tree", { name: "Files" })).toBeFocused();
});

test("a single click previews in one tab, a double click keeps it, pinned tabs outlast Close others", async ({
  page,
}) => {
  const dir = project({ "a.txt": "a\n", "b.txt": "b\n", "c.txt": "c\n" });
  await open(page, dir);
  const tabs = page.getByRole("tablist", { name: "Open files" });
  const row = (name: RegExp) => page.getByRole("treeitem", { name });
  const command = async (title: string) => {
    await page.keyboard.press("ControlOrMeta+Shift+P");
    await page.getByRole("combobox", { name: "Type a command" }).fill(title);
    await page.keyboard.press("Enter");
  };

  await row(/a\.txt/).click();
  await expect(tabs.getByRole("tab", { name: "a.txt, preview" })).toBeVisible();
  await row(/b\.txt/).click();
  await expect(tabs.getByRole("tab")).toHaveText(["b.txt"]);
  await row(/b\.txt/).dblclick();
  await expect(tabs.getByRole("tab", { name: "b.txt", exact: true })).toBeVisible();
  await row(/c\.txt/).click();
  await expect(tabs.getByRole("tab")).toHaveText(["b.txt", "c.txt"]);

  // Pinning moves the tab first and keeps it.
  await tabs.getByRole("tab", { name: /c\.txt/ }).hover();
  await tabs.getByRole("button", { name: "Pin c.txt", includeHidden: true }).click();
  await expect(tabs.getByRole("tab")).toHaveText(["c.txt", "b.txt"]);
  await expect(tabs.getByRole("tab", { name: "c.txt, pinned" })).toBeVisible();

  await row(/a\.txt/).dblclick();
  await command("Close other tabs");
  await expect(tabs.getByRole("tab")).toHaveText(["c.txt", "a.txt"]);
  await command("Close all tabs");
  await expect(tabs.getByRole("tab")).toHaveText(["c.txt"]);

  // A tab's menu, from the keyboard and with a right click.
  await row(/a\.txt/).dblclick();
  await row(/b\.txt/).dblclick();
  await tabs.getByRole("tab", { name: "a.txt", exact: true }).focus();
  await page.keyboard.press("Shift+F10");
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem", { name: "Pin tab" })).toBeVisible();
  await menu.getByRole("menuitem", { name: "Close to the right" }).click();
  await expect(tabs.getByRole("tab")).toHaveText(["c.txt", "a.txt"]);
  await tabs.getByRole("tab", { name: "c.txt, pinned" }).click({ button: "right" });
  await expect(menu.getByRole("menuitem", { name: "Unpin tab" })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Close to the right" })).toBeVisible();
  await menu.getByRole("menuitem", { name: "Close others" }).click();
  await expect(tabs.getByRole("tab")).toHaveText(["c.txt"]);

  // Dragging reorders; a tab dropped before a pinned one is pinned too.
  await row(/a\.txt/).dblclick();
  await row(/b\.txt/).dblclick();
  const tab = (name: RegExp) => tabs.getByRole("tab", { name });
  await tab(/b\.txt/).dragTo(tab(/a\.txt/), { targetPosition: { x: 4, y: 8 } });
  await expect(tabs.getByRole("tab")).toHaveText(["c.txt", "b.txt", "a.txt"]);
  await tab(/a\.txt/).dragTo(tab(/c\.txt/), { targetPosition: { x: 4, y: 8 } });
  await expect(tabs.getByRole("tab")).toHaveText(["a.txt", "c.txt", "b.txt"]);
  await expect(tabs.getByRole("tab", { name: "a.txt, pinned" })).toBeVisible();

  await page.reload();
  await expect(tabs.getByRole("tab")).toHaveText(["a.txt", "c.txt", "b.txt"]);
  await expect(tabs.getByRole("tab", { name: "c.txt, pinned" })).toBeVisible();
});

test("the keyboard keeps a preview tab, and a focused tab shows its actions", async ({ page }) => {
  const dir = project({ "a.txt": "a\n", "b.txt": "b\n" });
  await open(page, dir);
  const tabs = page.getByRole("tablist", { name: "Open files" });
  const tree = page.getByRole("tree", { name: "Files" });
  await expect(page.getByRole("treeitem", { name: /a\.txt/ })).toBeVisible();
  await tree.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(tabs.getByRole("tab", { name: "a.txt, preview" })).toBeVisible();
  // Enter on the file already shown keeps it, as a double click does.
  await tree.focus();
  await page.keyboard.press("Enter");
  await expect(tabs.getByRole("tab", { name: "a.txt", exact: true })).toBeVisible();
  await tree.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(tabs.getByRole("tab")).toHaveText(["a.txt", "b.txt"]);
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("combobox", { name: "Type a command" }).fill("Keep tab");
  await page.keyboard.press("Enter");
  await expect(tabs.getByRole("tab", { name: "b.txt", exact: true })).toBeVisible();

  // A focused tab shows Pin and Close with the pointer away, as a hovered one does.
  await page.mouse.move(0, 0);
  await tabs.getByRole("tab", { name: "b.txt", exact: true }).focus();
  await page.keyboard.press("ArrowLeft");
  await expect(tabs.getByRole("tab", { name: "a.txt", exact: true })).toBeFocused();
  await expect(tabs.getByRole("button", { name: "Pin a.txt", includeHidden: true })).toHaveCSS("opacity", "1");
  await expect(tabs.getByRole("button", { name: "Close a.txt", includeHidden: true })).toHaveCSS("opacity", "1");
  await expect(tabs.getByRole("button", { name: "Pin b.txt", includeHidden: true })).toHaveCSS("opacity", "0");
});

test("Edit sits in the file's bar and switches modes, and focus follows into the viewer", async ({ page }) => {
  const dir = project({ "sub/notes.txt": "hello\n" });
  await open(page, dir);
  await page.getByRole("treeitem", { name: /sub/ }).click();
  await page.getByRole("treeitem", { name: /notes\.txt/ }).click();
  const tabs = page.getByRole("tablist", { name: "Open files" });
  const main = page.getByTestId("main");
  // Typing into the read-only view says how to edit.
  await page.locator("#ovra-tabpanel .content").focus();
  await page.keyboard.type("x");
  await expect(main.getByText(/^This is a read-only view\. Press Edit/)).toBeVisible();
  await main.getByRole("button", { name: /^Edit/ }).click();
  const content = main.locator(".cm-content");
  await expect(content).toBeFocused();
  await expect(content).toHaveAccessibleName("Edit sub/notes.txt");
  await page.keyboard.type("x");
  await page.keyboard.press("ControlOrMeta+E");
  await expect(main.getByRole("button", { name: /^Edit/ })).toBeVisible();
  await expect(page.locator("#ovra-tabpanel .content")).toBeFocused();

  // The close question names the file by its path.
  await tabs.getByRole("button", { name: /Close notes\.txt/, includeHidden: true }).click();
  await expect(page.getByRole("dialog", { name: "Save changes to sub/notes.txt before closing?" })).toBeVisible();
});

test("unsaved text survives a reload and an outside change becomes a conflict", async ({ page }) => {
  const dir = project({ "notes.txt": "first\n" });
  await open(page, dir);
  await page.getByRole("treeitem", { name: /notes\.txt/ }).click();
  await page.keyboard.press("ControlOrMeta+E");
  const content = page.getByTestId("main").locator(".cm-content");
  await expect(content).toBeFocused();
  await page.keyboard.type("draft ");
  await expect(page.getByTestId("editor-dirty")).toBeVisible();

  await page.reload();
  await expect(page.getByTestId("project-name")).toContainText(basename(dir));
  const tabs = page.getByRole("tablist", { name: "Open files" });
  await expect(tabs.getByRole("tab", { name: "notes.txt, unsaved changes" })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+E");
  await expect(content).toContainText("draft first");

  writeFileSync(join(dir, "notes.txt"), "written by an agent\n");
  const alert = page.getByRole("alert");
  await expect(alert).toContainText("notes.txt changed on disk while you have unsaved changes");
  await expect(content).toContainText("draft first");
  await alert.getByRole("button", { name: "Overwrite with my version" }).click();
  await expect(alert).toBeHidden();
  expect(readFileSync(join(dir, "notes.txt"), "utf8")).toBe("draft first\n");
});

test("the viewer shows unsaved text, and binary files explain why they cannot be edited", async ({ page }) => {
  const dir = project({ "readme.md": "# Saved title\n" });
  writeFileSync(join(dir, "logo.png"), Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0]));
  await open(page, dir);
  await page.getByRole("treeitem", { name: /readme\.md/ }).click();
  await page.keyboard.press("ControlOrMeta+E");
  const content = page.getByTestId("main").locator(".cm-content");
  await expect(content).toBeFocused();
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("Draft paragraph");
  await page.keyboard.press("ControlOrMeta+E");
  const main = page.getByTestId("main");
  await expect(main.getByTestId("unsaved-preview")).toHaveText("Showing unsaved changes from the editor.");
  await expect(main.getByText("Draft paragraph")).toBeVisible();

  // Mod+S in view mode saves the unsaved text instead of the browser's "Save page".
  await page.keyboard.press("ControlOrMeta+S");
  await expect(main.getByTestId("unsaved-preview")).toBeHidden();
  expect(readFileSync(join(dir, "readme.md"), "utf8")).toContain("Draft paragraph");

  await page.getByRole("treeitem", { name: /logo\.png/ }).click();
  await page.keyboard.press("ControlOrMeta+E");
  await expect(page.getByRole("alert").filter({ hasText: "logo.png" })).toHaveText(
    "logo.png is a binary file and cannot be edited here.",
  );
  await expect(content).toBeHidden();
});

for (const width of [640, 512]) {
  test(`at 200% zoom (${width} px wide) the main area keeps a usable width`, async ({ page }) => {
    const dir = project({ "a.txt": "alpha\n" });
    // Open first: in a window this small, approvals other tests' agents wait
    // on (they share the daemon) can cover the start page's form.
    await open(page, dir);
    await page.setViewportSize({ width, height: 400 });
    await page.getByRole("treeitem", { name: /a\.txt/ }).click();
    const main = await page.getByTestId("main").boundingBox();
    expect(main?.width).toBeGreaterThanOrEqual(300);
    const scroll = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(scroll).toBeLessThanOrEqual(0);
  });
}
