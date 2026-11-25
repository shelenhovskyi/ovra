import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

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

  await page.getByTestId("open-servers").click();
  const dialog = page.getByRole("dialog", { name: "Servers" });
  await dialog.getByRole("button", { name: "Start web", exact: true }).click();
  const server = dialog.getByTestId("server");
  await expect(server.getByTestId("server-status")).toHaveText("ready");
  await expect(server).toContainText("port 5173");
  await expect(server).toContainText("started by you");
  await server.getByRole("button", { name: "Output" }).click();
  await expect(dialog.getByRole("log", { name: "Output of web" })).toContainText("ready in 1 ms on port 5173");

  await dialog.getByRole("button", { name: "Stop web" }).click();
  await expect(server.getByTestId("server-status")).toHaveText(/exited/);
  await expect(dialog.getByRole("button", { name: "Start web", exact: true })).toBeEnabled();
});
