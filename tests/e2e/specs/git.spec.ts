import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "../test";

function daemonUrl(): string {
  const raw = process.env.OVRA_URL;
  if (!raw) throw new Error("OVRA_URL is not set; globalSetup did not run");
  return raw;
}

/** Without the machine's global config, so its hooks or signing cannot interfere. */
const GIT_ENV = { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1" };

function git(dir: string, ...args: string[]): string {
  return execFileSync("git", args, { cwd: dir, encoding: "utf8", env: GIT_ENV });
}

const made: string[] = [];
test.afterAll(() => {
  for (const dir of made) rmSync(dir, { recursive: true, force: true });
});

/** A repository with one commit on `main` and its own identity. */
function repo(): string {
  const dir = mkdtempSync(join(tmpdir(), "ovra-git-"));
  made.push(dir);
  git(dir, "init", "-q", "-b", "main");
  git(dir, "config", "user.name", "E2E");
  git(dir, "config", "user.email", "e2e@example.invalid");
  git(dir, "config", "commit.gpgsign", "false");
  const lines = Array.from({ length: 30 }, (_, i) => `line ${i + 1}`);
  writeFileSync(join(dir, "notes.txt"), `${lines.join("\n")}\n`);
  writeFileSync(join(dir, "README.md"), "Hello\n");
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "-m", "First commit");
  return dir;
}

async function open(page: Page, dir: string): Promise<void> {
  await page.goto(daemonUrl());
  const projects = page.getByTestId("project-list");
  // Without a project the git shortcut goes to where a folder is opened.
  await expect(projects.getByRole("textbox", { name: "Open a folder" })).toBeVisible();
  await page.keyboard.press("Alt+Shift+G");
  await expect(projects.getByRole("textbox", { name: "Open a folder" })).toBeFocused();
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
}

test("stage a file and one hunk, commit, and switch to a new branch", async ({ page }) => {
  const dir = repo();
  const lines = readFileSync(join(dir, "notes.txt"), "utf8").split("\n");
  lines[1] = "line 2 changed";
  lines[27] = "line 28 changed";
  writeFileSync(join(dir, "notes.txt"), lines.join("\n"));
  writeFileSync(join(dir, "new.txt"), "new\n");
  await open(page, dir);

  await page.keyboard.press("Alt+Shift+G");
  const panel = page.getByTestId("git");
  await expect(panel.getByRole("textbox", { name: "Commit message" })).toBeFocused();
  await expect(panel.getByTestId("git-branch")).toHaveAccessibleName("Branch: main");
  const changes = panel.getByTestId("git-changes");
  await expect(changes.getByRole("button", { name: "Stage new.txt" })).toBeVisible();
  // A row's hover slot also shows while the row has focus, for keyboard users.
  const row = changes.locator('[data-row][data-path="notes.txt"]');
  const slot = row.locator("xpath=..").getByTestId("git-row-actions");
  await expect(slot).toHaveCSS("opacity", "0");
  await row.focus();
  await expect(slot).toHaveCSS("opacity", "1");

  await changes.getByRole("button", { name: "Stage new.txt" }).click();
  await expect(panel.getByTestId("git-staged").getByRole("button", { name: "Unstage new.txt" })).toBeVisible();

  // A row shows its diff as the selected tab. Only the first of the two hunks of notes.txt.
  await row.click();
  await expect(page.getByRole("tab", { name: "notes.txt (diff)", selected: true })).toBeVisible();
  const diff = page.getByRole("tabpanel", { name: "Diff of notes.txt" }).getByTestId("git-diff");
  const hunks = diff.locator("[data-hunk-action]");
  await expect(hunks).toHaveCount(2);
  // The diff bar keeps one action; Stage hunk shows on hover and on focus.
  await expect(diff.locator("header").getByRole("button")).toHaveText(["Open file"]);
  await expect(hunks.first()).toHaveCSS("opacity", "0");
  await hunks.first().focus();
  await expect(hunks.first()).toHaveCSS("opacity", "1");
  await hunks.first().click();
  await expect.poll(() => git(dir, "diff", "--cached", "--name-only")).toBe("new.txt\nnotes.txt\n");
  expect(git(dir, "diff", "--", "notes.txt")).toContain("+line 28 changed");
  // Escape closes the diff and goes back to the row.
  await page.keyboard.press("Escape");
  await expect(diff).toBeHidden();
  await expect(row).toBeFocused();

  await panel.getByRole("textbox", { name: "Commit message" }).fill("Add new.txt and fix line 2");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(panel.getByTestId("git-committed")).toContainText("Add new.txt and fix line 2");
  expect(git(dir, "log", "-1", "--format=%s")).toBe("Add new.txt and fix line 2\n");
  expect(git(dir, "status", "--porcelain")).toBe(" M notes.txt\n");

  await panel.getByTestId("git-branch").click();
  await panel.getByRole("button", { name: "New branch…" }).click();
  await panel.getByRole("textbox", { name: "New branch name" }).fill("feature/e2e");
  await panel.getByRole("button", { name: "Create and switch" }).click();
  await expect(panel.getByTestId("git-branch")).toHaveAccessibleName("Branch: feature/e2e");
  expect(git(dir, "branch", "--show-current")).toBe("feature/e2e\n");
});

test("the editor marks changed lines against the index", async ({ page }) => {
  const dir = repo();
  await open(page, dir);
  await page.getByRole("treeitem", { name: /notes\.txt/ }).click();
  await page.keyboard.press("ControlOrMeta+E");
  const main = page.getByTestId("main");
  const text = main.locator(".cm-content");
  await expect(text).toContainText("line 30");
  await text.click();
  await page.keyboard.press("ControlOrMeta+Home");
  await page.keyboard.type("added line\n");
  const marker = main.locator('[data-testid="git-marker"][data-change="added"]');
  await expect(marker).toHaveCount(1);
  await expect(marker).toBeVisible();
  expect((await marker.boundingBox())?.width).toBeGreaterThan(0);
  // The cursor is on line 2; the next change wraps around to line 1.
  await expect(main.locator(".cm-activeLine")).toHaveText("line 1");
  await page.keyboard.press("Alt+F5");
  await expect(main.locator(".cm-activeLine")).toHaveText("added line");
});
