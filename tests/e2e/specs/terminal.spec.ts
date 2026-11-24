import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

test("terminals open from the keyboard, run commands in tabs and hide without ending", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-terminal-"));

  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("tree", { name: "Files" })).toBeVisible();

  const panel = page.getByRole("region", { name: "Terminal panel" });
  await expect(panel).toBeHidden();
  await page.keyboard.press("Control+Backquote");
  await expect(panel).toBeVisible();
  const tabs = panel.getByRole("tablist", { name: "Terminals" });
  await expect(tabs.getByRole("tab")).toHaveText(["Terminal 1"]);

  // The shell computes the answer, so the tty echo of the command alone cannot match.
  const first = panel.getByRole("tabpanel", { name: "Terminal 1" });
  await page.keyboard.type("echo ovra-$((1+1))\n");
  await expect(first.locator(".xterm-rows")).toContainText("ovra-2");

  await page.keyboard.press("Control+Shift+Backquote");
  await expect(tabs.getByRole("tab")).toHaveText(["Terminal 1", "Terminal 2"]);
  await expect(tabs.getByRole("tab", { name: "Terminal 2" })).toHaveAttribute("aria-selected", "true");
  const second = panel.getByRole("tabpanel", { name: "Terminal 2" });
  await page.keyboard.type("echo second-$((2+3))\n");
  await expect(second.locator(".xterm-rows")).toContainText("second-5");

  // Focus is in the terminal, so the toggle hides the panel; the shells keep running.
  await page.keyboard.press("Control+Backquote");
  await expect(panel).toBeHidden();
  await page.keyboard.press("Control+Backquote");
  await expect(second.locator(".xterm-rows")).toContainText("second-5");

  // Typing `exit` ends the shell; the tab stays and says so.
  await page.keyboard.type("exit\n");
  await expect(tabs.getByRole("tab", { name: /Terminal 2/ })).toHaveText("Terminal 2 (exited)");
});
