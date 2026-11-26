import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, loadProject, test } from "../test";

test("split right shows one text in two groups; an edit on the right saves, a reload keeps both, and a group closes with its last tab", async ({
  page,
}) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-split-"));
  writeFileSync(join(dir, "a.txt"), "alpha\n");
  writeFileSync(join(dir, "b.txt"), "beta\n");
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  await loadProject(page, dir, url);
  await page.getByRole("treeitem", { name: /a\.txt/ }).dblclick();
  await page.keyboard.press("ControlOrMeta+E");

  const main = page.getByTestId("main");
  const left = main.locator('[data-group="0"]');
  const right = main.locator('[data-group="1"]');
  const strips = page.getByRole("tablist", { name: "Open files" });
  await expect(left.locator(".cm-content")).toBeFocused();
  await page.keyboard.press("ControlOrMeta+Backslash");
  await expect(strips).toHaveCount(2);
  const rightText = right.locator(".cm-content");
  await expect(rightText).toBeFocused();
  // Each group is narrow at 1280 px: its bar keeps one line, without the key caps.
  const viewButton = right.getByRole("button", { name: /^View/ });
  await expect(viewButton.locator("kbd")).toBeHidden();
  expect((await viewButton.boundingBox())?.height).toBeLessThan(34);

  // One buffer: typing on the right shows on the left, and both tabs say it is unsaved.
  await page.keyboard.press("ControlOrMeta+End");
  await page.keyboard.type("gamma");
  await expect(left.locator(".cm-content")).toContainText("gamma");
  await expect(strips.getByRole("tab", { name: "a.txt, unsaved changes" })).toHaveCount(2);
  await page.keyboard.press("ControlOrMeta+S");
  await expect.poll(() => readFileSync(join(dir, "a.txt"), "utf8")).toBe("alpha\ngamma");
  await expect(strips.getByRole("tab", { name: "a.txt", exact: true })).toHaveCount(2);

  // Mod+1 (Mod+Alt+1 in a browser tab) focuses the left group, where the tree opens files.
  await page.keyboard.press("ControlOrMeta+Alt+1");
  await expect(left.locator(".cm-content")).toBeFocused();
  await page.getByRole("treeitem", { name: /b\.txt/ }).click();
  await expect(strips.first().getByRole("tab", { name: /b\.txt/ })).toHaveAttribute("aria-selected", "true");
  await expect(right.locator(".cm-content")).toContainText("gamma");

  // Both groups come back after a reload.
  await page.reload();
  await expect(strips).toHaveCount(2);
  await expect(strips.first().getByRole("tab")).toHaveText(["a.txt", "b.txt"]);
  await expect(strips.nth(1).getByRole("tab")).toHaveText(["a.txt"]);

  // Closing the right group's only tab closes the group.
  await strips.nth(1).getByRole("button", { name: "Close a.txt" }).click();
  await expect(strips).toHaveCount(1);
  await expect(right).toHaveCount(0);
});
