import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "../test";

function daemonUrl(): URL {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return new URL(raw);
}

test("UI connects to the headless core", async ({ page }) => {
  await page.goto(daemonUrl().href);
  await expect(page.getByRole("heading", { name: "Ovra" })).toBeVisible();
  await expect(page.getByTestId("status")).toHaveText(/^Connected to ovra \d+\.\d+\.\d+ \(headless\)$/);
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

test("a wrong token is refused", async ({ page }) => {
  const url = daemonUrl();
  url.searchParams.set("token", "wrong");
  await page.goto(url.href);
  await expect(page.getByTestId("status")).toHaveText(/^Core unavailable/);
  // The server answers but refuses the token, as after a restart: say so and stop retrying.
  await expect(page.getByRole("heading", { name: "Ovra core was restarted" })).toBeVisible();
});
