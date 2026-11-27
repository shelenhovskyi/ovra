import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, loadProject, test } from "../test";

test("terminals open from the keyboard, run commands in tabs and hide without ending", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-terminal-"));

  await loadProject(page, dir, url);
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

  await loadProject(page, dir, url);
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

test("terminal tabs come back on their shells after a reload, and the panel resizes", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-terminal-"));

  await loadProject(page, dir, url);
  await expect(page.getByRole("tree", { name: "Files" })).toBeVisible();
  const panel = page.getByRole("region", { name: "Terminal panel" });
  await page.keyboard.press("Control+Backquote");
  await page.keyboard.type("export KEPT=still-$((3+4))\n");
  await page.keyboard.type("echo $KEPT\n");
  const rows = panel.getByRole("tabpanel", { name: "Terminal 1" }).locator(".xterm-rows");
  await expect(rows).toContainText("still-7");

  // The handle above the panel resizes it from the keyboard.
  const handle = page.getByRole("separator", { name: "Resize the bottom panel" });
  const before = (await panel.boundingBox())?.height ?? 0;
  await handle.focus();
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await expect.poll(async () => (await panel.boundingBox())?.height ?? 0).toBeGreaterThan(before + 20);
  const resized = (await panel.boundingBox())?.height ?? 0;

  await page.reload();
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("tab")).toHaveText(["Terminal 1"]);
  await expect(rows).toContainText("still-7");
  // The same shell: what it was told before the reload, it still knows.
  await panel.getByRole("textbox", { name: "Terminal input" }).focus();
  await page.keyboard.type("echo again-$KEPT\n");
  await expect(rows).toContainText("again-still-7");
  expect(Math.abs(((await panel.boundingBox())?.height ?? 0) - resized)).toBeLessThan(2);
});

test("a path in the output opens on Ctrl+click, and Ctrl+Shift+F finds in the output", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-terminal-"));
  mkdirSync(join(dir, "src"));
  writeFileSync(join(dir, "src/main.rs"), "fn main() {\n    let x = 1;\n}\n");

  await loadProject(page, dir);
  await expect(page.getByRole("tree", { name: "Files" })).toBeVisible();
  await page.keyboard.press("Control+Backquote");
  const panel = page.getByRole("tabpanel", { name: "Terminal 1" });
  const rows = panel.locator(".xterm-rows");
  // printf builds the path, so only the output, not the typed line, names the file.
  await page.keyboard.type("printf 'at src/%s.rs:2:5 found-%s\\n' main $((1+1))\n");
  await expect(rows).toContainText("at src/main.rs:2:5 found-2");

  const spot = await rows.evaluate((root, needle) => {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const at = node.textContent?.indexOf(needle) ?? -1;
      if (at < 0) continue;
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + needle.length);
      const box = range.getBoundingClientRect();
      return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    }
    return null;
  }, "src/main.rs:2:5");
  if (!spot) throw new Error("the path is not on screen");
  await page.mouse.move(spot.x, spot.y);
  await expect(panel.getByTestId("terminal-view")).toHaveAttribute("title", "Open file (Ctrl+click)");
  await page.keyboard.down("Control");
  await page.mouse.click(spot.x, spot.y);
  await page.keyboard.up("Control");
  await expect(page.getByRole("tab", { name: "main.rs" })).toHaveAttribute("aria-selected", "true");
  // The cursor is at line 2, column 5: typing lands before `let`.
  const text = page.getByTestId("main").locator(".cm-content");
  await expect(text).toBeFocused();
  await page.keyboard.type("Z");
  await expect(text).toContainText("Zlet x = 1;");

  await panel.getByRole("textbox", { name: "Terminal input" }).focus();
  await page.keyboard.press("Control+Shift+F");
  const find = panel.getByRole("textbox", { name: "Find in terminal" });
  await expect(find).toBeFocused();
  await page.keyboard.type("found-2");
  await expect(panel.getByRole("search")).toContainText("1 of 1");
  await page.keyboard.press("Escape");
  await expect(find).toBeHidden();
  await expect(panel.getByRole("textbox", { name: "Terminal input" })).toBeFocused();
});
