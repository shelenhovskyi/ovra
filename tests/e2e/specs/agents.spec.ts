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
  await page.getByRole("textbox", { name: "Give Claude a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");

  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  // The fake agent runs without a sandbox, and the session and card say so.
  await expect(page.getByTestId("agent-sandbox")).toHaveText(
    "No sandbox: the agent can read and change anything your user can.",
  );
  await expect(approvals.getByTestId("approval-unsandboxed")).toBeVisible();
  // The card is in sight, so no toast repeats it.
  await expect(page.getByTestId("toast")).toHaveCount(0);
  // The task opens, ready for a reply; nothing moves focus to the request.
  await expect(page.getByRole("textbox", { name: "Reply to Claude" })).toBeFocused();
  // In a 1280x720 window the request, the header and the reply box all fit:
  // Send is not cut off, and the header is one line.
  const agents = page.getByTestId("agents");
  await expect(agents.getByRole("button", { name: "Send" })).toBeInViewport({ ratio: 1 });
  expect((await page.getByTestId("agents-capacity").boundingBox())?.height).toBeLessThan(24);

  // Y and N would type into the reply box: no key cap says otherwise until the card has focus.
  await expect(run.locator("kbd")).toHaveCount(2);
  for (const cap of await run.locator("kbd").all()) await expect(cap).toBeHidden();
  await page.keyboard.press("Alt+Shift+A");
  await expect(run).toBeFocused();
  for (const cap of await run.locator("kbd").all()) await expect(cap).toBeVisible();
  await arm(page);
  await expect(run.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("y");

  // Focus goes back to the reply box, and the next request is not focused
  // when it arrives: typing must not answer it. The shortcut reaches it.
  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Reply to Claude" })).toBeFocused();
  await page.keyboard.press("Alt+Shift+A");
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
  // Once the turn is over, a command and an edit each are one folded card: verb, mono object, result inside.
  await expect(transcript).toContainText(/Claude\s*Done\./);
  const ran = transcript.locator(".step summary", { hasText: "Ran ls -la" });
  await expect(ran).toBeVisible();
  await expect(ran.locator("code")).toHaveText("ls -la");
  await expect(transcript.getByTestId("tool-detail").first()).toBeHidden();
  await ran.click();
  await expect(transcript.getByTestId("tool-detail").first()).toBeVisible();
  await expect(transcript.locator(".step.failed summary")).toHaveText(/Edit README\.md\s*failed/);
});

test("a working agent's text streams in under a live row until the turn ends", async ({ page }) => {
  await openProject(page, "ovra-agents-stream-");
  // The fake agent stays mid-sentence for prompts that start with "Stream".
  await page.getByRole("textbox", { name: "Give Claude a task" }).fill("Stream the notes");
  await page.keyboard.press("ControlOrMeta+Enter");

  const transcript = page.getByTestId("transcript");
  await expect(transcript.getByTestId("streaming-text")).toHaveText(
    /Claude\s*Reading the code first, then I will run the tests/,
  );
  const live = transcript.getByTestId("live-activity");
  await expect(live).toHaveText("Writing");
  await page.clock.fastForward(3000);
  await expect(live).toHaveText(/Writing\s*· 3s/);

  await page.getByTestId("agent-session").getByRole("button", { name: "Interrupt" }).click();
  await expect(page.getByTestId("agent-session").getByRole("status")).toHaveText("Done");
  await expect(live).toHaveCount(0);
  await expect(transcript.getByTestId("streaming-text")).toHaveCount(0);
});

test("in a short window, the shortcut shows the whole request and the title counts what waits", async ({ page }) => {
  // About 200% zoom on a laptop: approvals sit under the main area.
  await page.setViewportSize({ width: 640, height: 400 });
  await openProject(page, "ovra-agents-short-");
  await page.getByRole("textbox", { name: "Give Claude a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");

  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  await expect(page).toHaveTitle("(1) Ovra");
  // The card is in sight: no toast repeats it.
  await expect(page.getByTestId("toast")).toHaveCount(0);
  await page.keyboard.press("Alt+Shift+A");
  await expect(run).toBeFocused();
  await expect(run.locator("[data-details]")).toBeInViewport({ ratio: 1 });
  // Who asks and the sandbox warning stay in sight above it (SEC-2).
  await expect(approvals.locator(".session")).toBeInViewport({ ratio: 1 });
  await expect(approvals.getByTestId("approval-unsandboxed")).toBeInViewport({ ratio: 1 });
  await arm(page);
  await expect(run.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("y");

  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeVisible();
  await page.keyboard.press("Alt+Shift+A");
  await expect(edit).toBeFocused();
  await arm(page);
  await page.keyboard.press("n");
  await expect(approvals).toContainText("Nothing waits for your approval.");
  await expect(page).toHaveTitle("Ovra");
});

test("at 1024x700 the request's answers are in sight and not covered", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 });
  await openProject(page, "ovra-agents-small-");
  await page.getByRole("textbox", { name: "Give Claude a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");
  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  // The command once, in mono; nothing repeats it.
  await expect(run).toContainText("List files");
  await expect(run).not.toContainText(/\bcommand\b|\bdescription\b/);
  // The group head is one line (once the project list is in: until then it warns it cannot check paths).
  await expect(approvals.getByTestId("approval-unknown-project")).toHaveCount(0);
  expect((await approvals.locator(".who").boundingBox())?.height).toBeLessThan(28);
  for (const name of ["Deny", "Allow once", "Deny with a message…"]) {
    const button = run.getByRole("button", { name, exact: name === "Deny" });
    await expect(button).toBeInViewport({ ratio: 1 });
    // Nothing is drawn over it: a click lands on the button.
    const box = await button.boundingBox();
    const hit = await page.evaluate(
      ([x, y]) => document.elementFromPoint(x ?? 0, y ?? 0)?.closest("button")?.textContent ?? "",
      [(box?.x ?? 0) + (box?.width ?? 0) / 2, (box?.y ?? 0) + (box?.height ?? 0) / 2],
    );
    expect(hit).toContain(name.replace("…", ""));
  }
  await arm(page);
  await run.getByRole("button", { name: "Deny", exact: true }).click();
  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeVisible();
  await arm(page);
  await edit.getByRole("button", { name: "Deny", exact: true }).click();
  await expect(approvals).toContainText("Nothing waits for your approval.");
});

test("reply to an agent, allow with the mouse and stop it while it waits", async ({ page }) => {
  await openProject(page, "ovra-agents-reply-");
  await page.getByRole("textbox", { name: "Give Claude a task" }).fill("Tidy up");
  await page.getByRole("button", { name: "Start", exact: true }).click();

  const approvals = page.getByTestId("approvals");
  // A click before the card arms shows the wait as a fill on Allow (NEW-8);
  // `click()` would wait out `aria-disabled`.
  const first = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(first).toBeVisible();
  // A little ahead of the page's clock: by the time pauseAt runs, "now"
  // may have moved on, and pausing in the past throws.
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 50));
  const box = await first.getByRole("button", { name: "Allow" }).boundingBox();
  if (box) await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await expect(first.getByTestId("approval-arming")).toBeVisible();
  await expect(first.getByRole("button", { name: "Allow" })).toBeInViewport({ ratio: 1 });
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

  await page.getByRole("textbox", { name: "Give Claude a task" }).fill("Write the notes");
  await page.getByTestId("agent-mode").click();
  await page.getByRole("menuitemradio", { name: /^Plan and split/ }).click();
  await page.getByRole("button", { name: "Start", exact: true }).click();

  // Each worker asks to write its notes; the person allows all three. One
  // card at a time, the others wait on one line.
  const approvals = page.getByTestId("approvals");
  await expect(approvals.getByTestId("approvals-more")).toHaveText("2 more requests");
  await expect(approvals.locator("[data-request]")).toHaveCount(1);
  // Hide gives the room back at once, and the toast says what waits.
  await approvals.getByRole("button", { name: "Hide" }).click();
  await expect(page.getByTestId("toast")).toContainText("3 requests need approval");
  expect((await approvals.boundingBox())?.height).toBeLessThan(48);
  await approvals.getByRole("button", { name: "Show" }).click();
  await expect(page.getByTestId("toast")).toHaveCount(0);
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
  await page.getByRole("textbox", { name: "Give Claude a task" }).fill("Fix the typo");
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
