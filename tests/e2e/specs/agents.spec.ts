import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "../test";

// The daemon runs with --fake-agent: every session asks to run `ls -la` and to
// edit README.md, then waits; each reply asks to run `git status`. A task's
// coordinator starts three workers, and each asks to write NOTES.md in its
// worktree, then asks for review.
// Approvals are shared by all projects, so these tests take turns.
test.describe.configure({ mode: "serial" });

/** Approvals wait 500 ms before they can be answered; skip that wait. */
async function arm(page: Page): Promise<void> {
  await page.clock.fastForward(600);
}

async function openProject(page: Page, prefix: string): Promise<void> {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), prefix));
  writeFileSync(join(dir, "README.md"), "teh readme\n");
  await page.clock.install();
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
}

/** Opens a new git repository with README.md committed; returns its folder. */
async function openGitProject(page: Page, prefix: string): Promise<string> {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), prefix));
  writeFileSync(join(dir, "README.md"), "teh readme\n");
  const git = (...args: string[]) =>
    execFileSync("git", ["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", ...args], { cwd: dir });
  git("init", "-q");
  git("add", ".");
  git("commit", "-q", "-m", "start");
  await page.clock.install();
  await page.goto(url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  return dir;
}

test("start an agent and answer its requests from the keyboard", async ({ page }) => {
  await openProject(page, "ovra-agents-keys-");
  await page.getByRole("textbox", { name: "What should Claude do?" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");

  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  // The fake agent runs without a sandbox, and the session and card say so.
  await expect(page.getByTestId("agent-sandbox")).toHaveText(
    "No sandbox: the agent can read and change anything your user can.",
  );
  await expect(run.getByTestId("approval-unsandboxed")).toBeVisible();
  await expect(page.getByTestId("notifications")).toContainText("Fix the typo needs approval: Run ls -la");
  // The task opens, ready for a reply; nothing moves focus to the request.
  await expect(page.getByRole("textbox", { name: "Reply to Claude" })).toBeFocused();
  // In a 1280x720 window the request, the header and the reply box all fit:
  // Send is not cut off, and the header is one line with room for its buttons.
  const agents = page.getByTestId("agents");
  await expect(agents.getByRole("button", { name: "Send" })).toBeInViewport({ ratio: 1 });
  const box = async (name: string) =>
    (await agents.getByRole("button", { name }).boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
  const [capacity, newTask, swap] = await Promise.all([
    page.getByTestId("agents-capacity").boundingBox(),
    box("New task"),
    box("Move Agents to the other side"),
  ]);
  expect(capacity?.height).toBeLessThan(24);
  expect(swap.x).toBeGreaterThanOrEqual(newTask.x + newTask.width);
  expect(Math.abs(swap.y - newTask.y)).toBeLessThan(4);

  await page.keyboard.press("Alt+Shift+A");
  await expect(run).toBeFocused();
  await arm(page);
  await expect(run.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("y");

  // The next request is not focused when it arrives: typing must not answer it.
  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeVisible();
  await expect(approvals.getByRole("heading", { name: /Approvals/ })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(edit).toBeFocused();
  await arm(page);
  await expect(edit.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("Shift+N");
  await expect(edit.getByRole("textbox", { name: "Message to the agent" })).toBeFocused();
  await page.keyboard.type("Leave the readme alone");
  await page.keyboard.press("ControlOrMeta+Enter");

  const session = page.getByTestId("agent-session");
  await expect(session.getByRole("status")).toHaveText("Done");
  await expect(approvals).toContainText("Nothing waits for your approval.");
  // The empty band sits above the Agents header, never over it.
  const band = await approvals.boundingBox();
  const header = await agents.getByRole("heading", { name: "Agents", exact: true }).boundingBox();
  expect((band?.y ?? 0) + (band?.height ?? 0)).toBeLessThanOrEqual(header?.y ?? 0);
  const transcript = page.getByTestId("transcript");
  await expect(transcript).toContainText(/Tool output \(1 line\)\s*done/);
  await expect(transcript).toContainText(/Tool failed \(1 line\)\s*Leave the readme alone/);
  await expect(transcript).toContainText(/Claude\s*Done\./);
});

test("in a short window, Review shows the whole request and the title counts what waits", async ({ page }) => {
  // About 200% zoom on a laptop: approvals sit under the main area.
  await page.setViewportSize({ width: 640, height: 400 });
  await openProject(page, "ovra-agents-short-");
  await page.getByRole("textbox", { name: "What should Claude do?" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");

  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  await expect(page).toHaveTitle("(1) Ovra");
  await page.getByTestId("notifications").getByRole("button", { name: "Review" }).click();
  await expect(run).toBeFocused();
  await expect(run.locator("dl")).toBeInViewport({ ratio: 1 });
  // Who asks and the sandbox warning stay in sight above it (SEC-2).
  await expect(run.locator(".session")).toBeInViewport({ ratio: 1 });
  await expect(run.getByTestId("approval-unsandboxed")).toBeInViewport({ ratio: 1 });
  await arm(page);
  await expect(run.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("y");

  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeVisible();
  const toasts = page.getByTestId("toast");
  await expect(toasts).toContainText(["needs approval: Edit README.md"]);
  await page.keyboard.press("ArrowDown");
  await expect(edit).toBeFocused();
  // The focused request is on screen: its toast no longer covers it.
  await expect(toasts).toHaveCount(0);
  await arm(page);
  await page.keyboard.press("n");
  await expect(approvals).toContainText("Nothing waits for your approval.");
  await expect(page).toHaveTitle("Ovra");
});

test("reply to an agent, allow with the mouse and stop it while it waits", async ({ page }) => {
  await openProject(page, "ovra-agents-reply-");
  await page.getByRole("textbox", { name: "What should Claude do?" }).fill("Tidy up");
  await page.getByRole("button", { name: "Start", exact: true }).click();

  const approvals = page.getByTestId("approvals");
  // A click before the card arms says why, next to the buttons (NEW-8);
  // `click()` would wait out `aria-disabled`.
  const first = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(first).toBeVisible();
  // A little ahead of the page's clock: by the time pauseAt runs, "now"
  // may have moved on, and pausing in the past throws.
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 50));
  const box = await first.getByRole("button", { name: "Allow" }).boundingBox();
  if (box) await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await expect(first.getByTestId("approval-wait")).toBeInViewport({ ratio: 1 });
  await page.clock.resume();
  for (const name of ["Run ls -la", "Edit README.md"]) {
    const card = approvals.getByRole("group", { name });
    await expect(card).toBeVisible();
    await arm(page);
    await expect(card.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
    await card.getByRole("button", { name: "Allow" }).click();
  }
  const session = page.getByTestId("agent-session");
  await expect(session.getByRole("status")).toHaveText("Done");

  await session.getByRole("textbox", { name: "Reply to Claude" }).fill("Check the status too");
  await session.getByRole("button", { name: "Send" }).click();
  const status = approvals.getByRole("group", { name: "Run git status" });
  await expect(status).toBeVisible();
  await expect(session.getByRole("status")).toHaveText("Needs approval");

  // Stopping denies what the agent still waits for.
  await session.getByRole("button", { name: "Stop" }).click();
  await expect(session.getByRole("status")).toHaveText("Stopped");
  await expect(status).toBeHidden();
  await expect(session.getByRole("textbox", { name: "Reply to Claude" })).toBeHidden();
  // The list says so too; Close takes it off.
  await session.getByRole("button", { name: "Tasks" }).click();
  const agents = page.getByTestId("agents-panel");
  await expect(agents.getByRole("button", { name: /Tidy up.*Stopped/ })).toBeVisible();
  await agents.getByRole("button", { name: "Close Tidy up" }).click();
  await expect(agents.getByTestId("agents-empty")).toBeVisible();

  // The audit log says what was decided and by whom, newest first.
  await page.getByTestId("more").click();
  await page.getByRole("menuitem", { name: "Audit log" }).click();
  const audit = page.getByRole("dialog", { name: "Audit log" });
  const entries = audit.getByTestId("audit-entry");
  await expect(entries).toHaveCount(4);
  await expect(entries.nth(0)).toContainText("Refused Bash");
  await expect(entries.nth(0)).toContainText("because the agent stopped or finished");
  await expect(entries.nth(1)).toContainText("Allowed Edit");
  await expect(entries.nth(1)).toContainText("SHA-256 of what it writes");
  await expect(entries.nth(2)).toContainText("Allowed by a person");
  await expect(entries.nth(3)).toContainText("Agent started");
  await expect(entries.nth(3)).toContainText("no sandbox");
  await page.keyboard.press("Escape");
  await expect(audit).toBeHidden();
});

test("a task's coordinator splits the work among three workers whose diffs wait for review", async ({ page }) => {
  // Workers need a git repository for their worktrees.
  await openGitProject(page, "ovra-agents-task-");

  await page.getByRole("textbox", { name: "What should Claude do?" }).fill("Write the notes");
  await page.getByRole("radio", { name: "Plan and split" }).check();
  await page.getByRole("button", { name: "Start", exact: true }).click();

  // Each worker asks to write its notes; the person allows all three.
  const approvals = page.getByTestId("approvals");
  for (let allowed = 0; allowed < 3; allowed++) {
    const card = approvals.getByRole("group", { name: "Write NOTES.md" }).first();
    await expect(card).toBeVisible();
    await arm(page);
    await expect(card.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
    await card.getByRole("button", { name: "Allow" }).click();
  }
  await expect(approvals).toContainText("Nothing waits for your approval.");

  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByRole("dialog", { name: "Review inbox" });
  const items = inbox.getByRole("navigation", { name: "Review items" }).getByRole("button");
  await expect(items).toHaveCount(3);
  for (const index of [0, 1, 2]) {
    await items.nth(index).click();
    const item = inbox.getByTestId("inbox-item");
    await expect(item).toContainText("Agent's summary: Wrote NOTES.md.");
    await expect(item.getByRole("list", { name: "Changed files" })).toContainText("NOTES.md");
    await expect(item.getByTestId("inbox-diff")).toContainText("Notes from a worker.");
  }
});

test("a direct session's protected files and git hooks need a look before approve", async ({ page }) => {
  const dir = await openGitProject(page, "ovra-agents-protected-");
  await page.getByRole("textbox", { name: "What should Claude do?" }).fill("Fix the typo");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  const approvals = page.getByTestId("approvals");
  for (const name of ["Run ls -la", "Edit README.md"]) {
    const card = approvals.getByRole("group", { name });
    await expect(card).toBeVisible();
    await arm(page);
    const deny = card.getByRole("button", { name: "Deny", exact: true });
    await expect(deny).toHaveAttribute("aria-disabled", "false");
    await deny.click();
  }
  await expect(page.getByTestId("agent-session").getByRole("status")).toHaveText("Done");

  // Not the agent's file tools: the person (here the test) writes these.
  writeFileSync(join(dir, ".envrc"), "echo hi\n");
  writeFileSync(join(dir, ".mcp.json"), "{}\n");
  writeFileSync(join(dir, ".git/hooks/pre-commit"), "#!/bin/sh\n");

  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByRole("dialog", { name: "Review inbox" });
  const entry = inbox.getByRole("navigation", { name: "Review items" }).getByRole("button");
  await expect(entry).toHaveCount(1);
  await expect(entry.getByTestId("item-protected")).toHaveText("2 files can run code or change agent rules");
  await expect(entry.getByTestId("item-git")).toHaveText("1 git hook or config file changed");
  await entry.click();
  const item = inbox.getByTestId("inbox-item");
  await expect(item.getByTestId("inbox-git")).toContainText("pre-commit (added)");
  await expect(item.getByTestId("inbox-git")).toContainText("leave them as they are");
  await expect(item.getByTestId("inbox-protected")).toContainText("Reject leaves .envrc, .mcp.json as they are");
  await expect(item.getByTestId("inbox-protected")).not.toContainText("Reject restores");

  // It opens on the first protected file; Keep changes waits for the other one.
  const approve = item.getByRole("button", { name: "Keep changes", exact: true });
  await expect(item.getByTestId("inbox-diff")).toContainText("echo hi");
  await expect(item.getByTestId("inbox-protected-why")).toContainText(".mcp.json");
  await expect(approve).toHaveAttribute("aria-disabled", "true");
  await item
    .getByRole("list", { name: "Changed files" })
    .getByRole("button", { name: /\.mcp\.json/ })
    .click();
  await expect(item.getByTestId("inbox-protected-why")).toBeHidden();
  await expect(approve).toHaveAttribute("aria-disabled", "false");
  await approve.click();
  await expect(item).toContainText("Kept");
  expect(existsSync(join(dir, ".envrc"))).toBe(true);
  expect(existsSync(join(dir, ".git/hooks/pre-commit"))).toBe(true);
});
