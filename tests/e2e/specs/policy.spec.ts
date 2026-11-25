import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

test("a project's policy rules are added in the policies dialog, refused inline and kept", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-policy-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  await page.getByTestId("more").click();
  await page.getByRole("menuitem", { name: "Policies" }).click();
  const dialog = page.getByRole("dialog", { name: "Project policies" });
  await expect(dialog.getByRole("heading", { name: "Project policies" })).toBeFocused();
  await expect(dialog.getByTestId("policy-empty")).toHaveText(/No rules yet/);

  // A tool that never takes a match turns Match off, and a match typed before is refused once.
  await dialog.getByLabel("Tool", { exact: true }).fill("WebFetch");
  await expect(dialog.getByLabel("Match (optional)")).toBeDisabled();
  await dialog.getByLabel("Tool", { exact: true }).fill("");
  await dialog.getByLabel("Match (optional)").fill("example.com");
  await dialog.getByLabel("Tool", { exact: true }).fill("WebFetch");
  await dialog.getByRole("combobox", { name: "Action" }).selectOption("deny");
  await dialog.getByRole("button", { name: "Add rule" }).click();
  await expect(dialog.getByRole("alert")).toHaveText(/^WebFetch takes no match/);

  await dialog.getByLabel("Tool", { exact: true }).fill("Bash");
  await dialog.getByLabel("Match (optional)").fill("git push");
  await dialog.getByRole("button", { name: "Add rule" }).click();
  await expect(dialog.getByTestId("policy-rule")).toHaveText(/Bash\(git push\)\s*Deny.*Advisory/);

  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  // A new page reads the rules from the core again.
  // The project opens by itself after a reload.
  await page.reload();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await page.getByTestId("more").click();
  await page.getByRole("menuitem", { name: "Policies" }).click();
  await expect(page.getByTestId("policy-rule")).toHaveText(/Bash\(git push\)/);
});
