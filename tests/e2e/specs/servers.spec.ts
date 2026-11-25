import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

test("the servers tab of the dock opens with its shortcut and says what runs", async ({ page }) => {
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

  await page.keyboard.press("ControlOrMeta+Alt+KeyS");
  const dock = page.getByTestId("servers");
  await expect(dock.getByRole("heading", { name: "Servers", exact: true })).toBeFocused();
  // Nothing runs yet (or there is no Linux sandbox): the tab says so.
  await expect(dock.getByText("No servers running").or(dock.getByTestId("servers-unavailable"))).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dock).toBeHidden();
});

test("a defined server starts, gets ready, shows its output and stops", async ({ page }) => {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), "ovra-servers-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  mkdirSync(join(dir, ".ovra"));
  // `ready` is the fake servers' preset that gets ready at once.
  writeFileSync(
    join(dir, ".ovra/servers.json"),
    JSON.stringify({ servers: [{ name: "web", command: ["ready"], port: 5173 }] }),
  );
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  // Defined and started in Settings > Servers; it runs in the dock's Servers tab.
  await page.keyboard.press("ControlOrMeta+K");
  await page.keyboard.type(">dev servers");
  await page.getByRole("option", { name: /Settings\s*Servers/ }).click();
  const settings = page.getByRole("region", { name: "Settings" });
  await settings.getByRole("button", { name: "Start web", exact: true }).click();
  const dialog = page.getByTestId("servers");
  const server = dialog.getByTestId("server");
  await expect(server.getByTestId("server-status")).toHaveText("ready");
  await expect(server).toContainText("port 5173");
  await expect(server).toContainText("started by you");
  await server.getByRole("button", { name: "Output" }).click();
  await expect(dialog.getByRole("log", { name: "Output of web" })).toContainText("ready in 1 ms on port 5173");

  await dialog.getByRole("button", { name: "Stop web" }).click();
  await expect(server.getByTestId("server-status")).toHaveText("stopped");
  await expect(settings.getByRole("button", { name: "Start web", exact: true })).toBeVisible();
});
