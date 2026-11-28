import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, loadProject, test } from "../test";

test("a project's rules are added in Settings, refused inline and kept", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-policy-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await loadProject(page, dir, url);
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  await page.getByTestId("more").click();
  await page.getByRole("menuitem", { name: "Settings…" }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("searchbox", { name: "Search settings" })).toBeFocused();
  await settings.getByRole("navigation", { name: "Settings panes" }).getByRole("button", { name: "Rules" }).click();
  const dialog = settings.getByRole("region", { name: "Rules" });
  await expect(dialog.getByTestId("policy-empty")).toHaveText(/No rules yet/);

  // A tool that never takes a match turns Match off, and a match typed before is refused once.
  await dialog.getByLabel("Tool", { exact: true }).fill("WebSearch");
  await expect(dialog.getByLabel("Match (optional)")).toBeDisabled();
  await dialog.getByLabel("Tool", { exact: true }).fill("");
  await dialog.getByLabel("Match (optional)").fill("example.com");
  await dialog.getByLabel("Tool", { exact: true }).fill("WebSearch");
  await dialog.getByRole("combobox", { name: "Action" }).selectOption("deny");
  await dialog.getByRole("button", { name: "Add rule" }).click();
  await expect(dialog.getByRole("alert")).toHaveText(/^WebSearch takes no match/);

  await dialog.getByLabel("Tool", { exact: true }).fill("Bash");
  await dialog.getByLabel("Match (optional)").fill("git push");
  await dialog.getByRole("button", { name: "Add rule" }).click();
  await expect(dialog.getByTestId("policy-rule")).toHaveText(/Bash\(git push\)\s*Deny.*Advisory/);

  // A click outside the window closes it.
  await page.mouse.click(4, 400);
  await expect(settings).toBeHidden();
  // A new page reads the rules from the core again; the project opens by
  // itself after a reload, and Mod+K finds the pane by another name.
  await page.reload();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+K");
  await page.keyboard.type(">permissions");
  await page.getByRole("option", { name: /Rules/ }).click();
  await expect(page.getByTestId("policy-rule")).toHaveText(/Bash\(git push\)/);
  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();
});
