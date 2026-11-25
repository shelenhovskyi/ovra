import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

test("the servers dialog opens with its shortcut and says what it can run", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-servers-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  mkdirSync(join(dir, ".ovra"));
  writeFileSync(
    join(dir, ".ovra/servers.json"),
    JSON.stringify({ servers: [{ name: "web", command: ["ready"], port: 5173 }] }),
  );
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  await page.keyboard.press("ControlOrMeta+Shift+KeyS");
  const dialog = page.getByRole("dialog", { name: "Servers" });
  await expect(dialog.getByRole("heading", { name: "Servers" })).toBeFocused();
  // Without servers in the core (or without the Linux sandbox) the dialog says why.
  await expect(dialog.getByTestId("server-def").or(dialog.getByTestId("servers-unavailable"))).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
});
