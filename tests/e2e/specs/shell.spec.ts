import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { expect, test } from "../test";

function daemonUrl(): URL {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return new URL(raw);
}

test("UI connects to the headless core", async ({ page }) => {
  await page.goto(daemonUrl().href);
  await expect(page.getByRole("heading", { name: "Ovra" })).toBeVisible();
  await expect(page.getByTestId("status")).toHaveText("Connected");
  await expect(page.getByTestId("status")).toHaveAttribute("title", /^ovra \d+\.\d+\.\d+ \(headless\)$/);
});

test("the shell shows its panels", async ({ page }) => {
  await page.goto(daemonUrl().href);
  await expect(page.getByTestId("sidebar")).toContainText("File tree");
  await expect(page.getByTestId("main")).toContainText("Open a project to see its files here.");
});

test("with no file open, the main area offers the next steps", async ({ page }) => {
  const dir = mkdtempSync(join(tmpdir(), "ovra-start-"));
  writeFileSync(join(dir, "README.md"), "readme\n");
  await page.goto(daemonUrl().href);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  const start = page.getByTestId("start");
  await expect(start).toContainText("Open a file from the tree on the left");
  // Headless mode runs in a browser tab, so the browser-safe key is shown.
  await start.getByRole("button", { name: /Find a file/ }).click();
  await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(start.getByRole("button", { name: /Find a file/ })).toContainText("Alt+P");
  await start.getByRole("button", { name: /Search in files/ }).click();
  await expect(page.getByRole("button", { name: "Search", pressed: true })).toBeVisible();
});

test("the project name switches projects, opens another folder and survives a reload", async ({ page }) => {
  const first = mkdtempSync(join(tmpdir(), "ovra-switch-a-"));
  const second = mkdtempSync(join(tmpdir(), "ovra-switch-b-"));
  writeFileSync(join(first, "a.md"), "a\n");
  writeFileSync(join(second, "b.md"), "b\n");
  await page.goto(daemonUrl().href);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(first);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /a\.md/ })).toBeVisible();

  // A browser tab has no folder dialog: Open a folder… asks for a path in the
  // menu, and the project stays open until the new folder opens.
  const name = page.getByTestId("project-name");
  await name.click();
  await page.getByRole("menuitem", { name: "Open a folder…" }).click();
  const field = page.getByRole("textbox", { name: "Folder to open" });
  await expect(field).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(field).toBeHidden();
  await expect(name).toBeFocused();
  await expect(page.getByRole("treeitem", { name: /a\.md/ })).toBeVisible();
  await name.click();
  await page.getByRole("menuitem", { name: "Open a folder…" }).click();
  await field.fill(second);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("treeitem", { name: /b\.md/ })).toBeVisible();

  // Back to the first one with the keyboard: the menu lists it under Recent projects.
  await name.focus();
  await page.keyboard.press("ArrowDown");
  const menu = page.getByRole("menu", { name: /switch project/ });
  await expect(menu.getByRole("menuitem").first()).toBeFocused();
  await menu.getByRole("menuitem", { name: basename(first) }).press("Enter");
  await expect(page.getByRole("treeitem", { name: /a\.md/ })).toBeVisible();
  await expect(name).toBeFocused();

  await page.reload();
  await expect(name).toContainText(basename(first));
  await expect(page.getByRole("treeitem", { name: /a\.md/ })).toBeVisible();

  // Closing the project is remembered too.
  await name.click();
  await page.getByRole("menuitem", { name: "Close project" }).click();
  await expect(projects).toBeVisible();
  await page.reload();
  await expect(projects).toBeVisible();
  await expect(name).toHaveText("No project open");
});

test("a wrong token is refused", async ({ page }) => {
  const url = daemonUrl();
  url.searchParams.set("token", "wrong");
  await page.goto(url.href);
  await expect(page.getByTestId("status")).toHaveText(/^Core unavailable/);
  // The server answers but refuses the token, as after a restart: say so and stop retrying.
  await expect(page.getByRole("heading", { name: "Ovra was restarted" })).toBeVisible();
});
