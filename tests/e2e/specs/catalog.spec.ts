import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, loadProject, test } from "../test";

test("see a project's skills and instructions and open one", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-catalog-"));
  mkdirSync(join(dir, ".claude", "skills", "release"), { recursive: true });
  writeFileSync(join(dir, "AGENTS.md"), "# Rules\n\nWrite English.\n");
  writeFileSync(
    join(dir, ".claude", "skills", "release", "SKILL.md"),
    "---\nname: release\ndescription: Cuts a release.\n---\n# Release steps\n",
  );
  writeFileSync(join(dir, ".claude", "skills", "release", "tag.sh"), "git tag\n");
  await loadProject(page, dir);

  await page.keyboard.press("ControlOrMeta+K");
  await page.getByRole("combobox", { name: "Search files, or type > for commands" }).fill(">skills");
  await expect(page.getByRole("option", { selected: true })).toContainText("Skills and instructions");
  await page.keyboard.press("Enter");

  const view = page.getByTestId("catalog");
  await expect(view.getByRole("heading", { name: /^Instructions/ })).toBeVisible();
  await view.getByRole("button", { name: "release" }).click();
  const detail = view.getByTestId("catalog-detail");
  await expect(detail).toContainText("Cuts a release.");
  await expect(detail).toContainText(".claude/skills/release/tag.sh");
  await expect(view.getByTestId("catalog-text")).toContainText("Release steps");
  await view.getByTestId("catalog-open").click();
  await expect(view).toHaveCount(0);
});
