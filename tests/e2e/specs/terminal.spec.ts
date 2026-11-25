import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

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

  // Restart brings a new shell into the same tab; Shift+Tab leaves the terminal for its tab.
  await second.getByRole("button", { name: "Restart" }).click();
  await expect(tabs.getByRole("tab", { name: "Terminal 2" })).toHaveText("Terminal 2");
  await expect(second.getByRole("textbox", { name: "Terminal input" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(tabs.getByRole("tab", { name: "Terminal 2" })).toBeFocused();
});

test("a terminal redraws output it missed from the core's scrollback", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-terminal-"));

  // Pass frames through, but lose terminal output while `dropping` (a client that fell behind).
  let dropping = false;
  let dropped = "";
  await page.routeWebSocket(/\/ws/, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((message) => {
      if (dropping && String(message).includes('"terminals.output"')) {
        dropped += String(message);
        return;
      }
      ws.send(message);
    });
  });

  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("tree", { name: "Files" })).toBeVisible();

  await page.keyboard.press("Control+Backquote");
  const rows = page.getByRole("tabpanel", { name: "Terminal 1" }).locator(".xterm-rows");
  await page.keyboard.type("echo ready-$((0+1))\n");
  await expect(rows).toContainText("ready-1");

  dropping = true;
  await page.keyboard.type("echo lost-$((1+1))\n");
  // Wait until the shell's answer was lost on the way (the typed line reads `lost-$((1+1))`).
  await expect.poll(() => dropped).toContain("lost-2");
  await expect(rows).not.toContainText("lost-2");
  dropping = false;

  // The next output skips seq, so the tab redraws and shows the lost lines once.
  await page.keyboard.type("echo back-$((2+2))\n");
  await expect(rows).toContainText("back-4");
  await expect(rows).toContainText("lost-2");
  await expect(rows.getByText("lost-2", { exact: false })).toHaveCount(1);
});
