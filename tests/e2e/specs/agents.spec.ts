import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import type { Page } from "@playwright/test";
import { startDaemon } from "../daemon";
import { addProject, clickWhenArmed, expect, expectAccessible, loadProject, stopAgents, tabTo, test } from "../test";

// The daemon runs with --fake-agent: every session asks to run `ls -la` and to
// edit README.md, then waits; each reply asks to run `git status`. A task's
// coordinator starts three workers, and each asks to write NOTES.md in its
// worktree, then asks for review.
// Approvals are shared by all projects, so these tests take turns, and each
// stops its agents when it ends: one that asks again later would put its card
// before the next test's own.
test.describe.configure({ mode: "serial" });
test.afterEach(({ page }) => stopAgents(page));

/** Approvals wait 500 ms before they can be answered; skip that wait. */
async function arm(page: Page): Promise<void> {
  await page.clock.fastForward(600);
}

async function openProject(page: Page, prefix: string, url = process.env.OVRA_URL): Promise<void> {
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), prefix));
  writeFileSync(join(dir, "README.md"), "teh readme\n");
  await page.clock.install();
  await loadProject(page, dir, url);
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
}

/** The text of `path`, or `null` while it does not exist. */
function readIfThere(path: string): string | null {
  return existsSync(path) ? readFileSync(path, "utf8") : null;
}

/** Opens a new git repository with README.md and `files` committed; returns its folder. */
async function openGitProject(page: Page, prefix: string, files: Record<string, string> = {}): Promise<string> {
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  const dir = mkdtempSync(join(tmpdir(), prefix));
  writeFileSync(join(dir, "README.md"), "teh readme\n");
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(join(dir, path, ".."), { recursive: true });
    writeFileSync(join(dir, path), text);
  }
  const git = (...args: string[]) => execFileSync("git", args, { cwd: dir });
  git("init", "-q", "-b", "main");
  // In the repository, not on the command line: Ovra's own git commits a
  // pull request's work too, and CI has no identity of its own.
  git("config", "user.name", "e2e");
  git("config", "user.email", "e2e@example.invalid");
  git("add", ".");
  git("commit", "-q", "-m", "start");
  await page.clock.install();
  await loadProject(page, dir, url);
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  return dir;
}

test("plan first holds the agent's changes, and a reply queued meanwhile goes when its turn ends", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("ovra.agents.prefs", JSON.stringify({ planFirst: true })));
  const dir = mkdtempSync(join(tmpdir(), "ovra-agents-plan-"));
  writeFileSync(join(dir, "README.md"), "teh readme\n");
  const url = process.env.OVRA_URL;
  if (!url) throw new Error("OVRA_URL is not set; globalSetup did not run");
  await page.clock.install();
  await loadProject(page, dir, url);
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Plan the typo fix");
  await expect(page.getByRole("checkbox", { name: "Plan first" })).toBeChecked();
  await page.getByRole("button", { name: "Start", exact: true }).click();

  const approvals = page.getByTestId("approvals");
  const session = page.getByTestId("agent-session");
  // Even a command the fake agent's settings would allow comes to the person.
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  await expect(session.getByTestId("plan-bar")).toHaveText(
    "Planning. The agent changes no files until you approve its plan.",
  );
  await session.getByRole("textbox", { name: "Reply" }).fill("Also look at the docs");
  await session.getByRole("button", { name: "Queue" }).click();
  await expect(session.getByTestId("queued-message")).toHaveText(/Also look at the docs/);

  await arm(page);
  await run.getByRole("button", { name: "Allow" }).click();
  // Its edit is refused without a card; the turn ends and the queued reply
  // starts the next one, which asks for `git status`.
  const status = approvals.getByRole("group", { name: "Run git status" });
  await expect(status).toBeVisible();
  await expect(session.getByTestId("queued-message")).toHaveCount(0);
  await expect(approvals.getByRole("group", { name: "Edit README.md" })).toHaveCount(0);
  await arm(page);
  await status.getByRole("button", { name: "Allow" }).click();
  await expect(session.getByRole("status")).toHaveText("Done");
  expect(readFileSync(join(dir, "README.md"), "utf8")).toBe("teh readme\n");

  await session.getByRole("button", { name: "Approve plan" }).click();
  await expect(session.getByTestId("plan-bar")).toHaveCount(0);
  await expect(approvals.getByRole("group", { name: "Run git status" })).toBeVisible();
});

test("start an agent and answer its requests from the keyboard", async ({ page }) => {
  await openProject(page, "ovra-agents-keys-");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");

  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  // The fake agent runs without a sandbox, and the session and card say so.
  await expect(page.getByTestId("agent-sandbox")).toHaveText("No sandbox: the agent can change anything you can");
  await expect(approvals.getByTestId("approval-unsandboxed")).toBeVisible();
  // The card is in sight, so no toast repeats it. (Toasts about other
  // tests' agents can show up: they share the daemon.)
  await expect(page.getByTestId("toast").filter({ hasText: "approval" })).toHaveCount(0);
  // The task opens, ready for a reply; nothing moves focus to the request.
  await expect(page.getByRole("textbox", { name: "Reply" })).toBeFocused();
  // In a 1280x720 window the request, the header and the reply box all fit:
  // Queue (Send once it is done) is not cut off, and the header is one line.
  const agents = page.getByTestId("agents");
  await expect(agents.getByRole("button", { name: "Queue" })).toBeInViewport({ ratio: 1 });
  expect((await page.getByTestId("agents-capacity").boundingBox())?.height).toBeLessThan(24);

  // Y and N would type into the reply box: no key cap says otherwise until the card has focus.
  await expect(run.locator("kbd")).toHaveCount(2);
  for (const cap of await run.locator("kbd").all()) await expect(cap).toBeHidden();
  await page.keyboard.press("Alt+Shift+A");
  await expect(run).toBeFocused();
  for (const cap of await run.locator("kbd").all()) await expect(cap).toBeVisible();
  await expectAccessible(page, "with a request waiting");
  await arm(page);
  await expect(run.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("y");

  // Focus waits on the Approvals heading, not in the reply box, and the next
  // request is not focused when it arrives: a second Y neither answers it
  // nor types into the reply box, and says why. The shortcut reaches it.
  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeVisible();
  await expect(approvals.getByRole("heading", { name: /^Approvals/ })).toBeFocused();
  await page.keyboard.press("y");
  await expect(approvals.getByTestId("approvals-notice")).toHaveText(
    "New request: press Alt+Shift+A to read it first.",
  );
  await expect(page.getByRole("textbox", { name: "Reply" })).toHaveValue("");
  await expect(edit).toBeVisible();
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
  // Denied with a message: the agent got the message, yet the step says Denied, not failed.
  await expect(transcript.locator(".step summary", { hasText: "Denied" })).toHaveText(/Edit README\.md\s*Denied/);
  await expect(transcript.locator(".step.failed")).toHaveCount(0);
  await expectAccessible(page, "with the transcript open");
});

test("a working agent's text streams in under a live row until the turn ends", async ({ page }) => {
  await openProject(page, "ovra-agents-stream-");
  // The fake agent stays mid-sentence for prompts that start with "Stream".
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Stream the notes");
  await page.keyboard.press("ControlOrMeta+Enter");

  const transcript = page.getByTestId("transcript");
  await expect(transcript.getByTestId("streaming-text")).toHaveText(
    /Claude\s*Reading the code first, then I will run the tests/,
  );
  const live = transcript.getByTestId("live-activity");
  await expect(live).toHaveText("Writing");
  await page.clock.fastForward(3000);
  await expect(live).toHaveText(/Writing\s*· 3s/);

  await page.getByTestId("agent-session").getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.getByTestId("agent-session").getByRole("status")).toHaveText("Done");
  await expect(live).toHaveCount(0);
  await expect(transcript.getByTestId("streaming-text")).toHaveCount(0);
});

test("in a short window, the shortcut shows the whole request and the title counts what waits", async ({ page }) => {
  // About 200% zoom on a laptop: approvals sit under the main area.
  await page.setViewportSize({ width: 640, height: 400 });
  await openProject(page, "ovra-agents-short-");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");

  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  await expect(page).toHaveTitle("(1) Ovra");
  // The card is in sight: no toast repeats it.
  await expect(page.getByTestId("toast").filter({ hasText: "approval" })).toHaveCount(0);
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
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");
  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  // The command once, in mono; nothing repeats it.
  await expect(run).toContainText("List files");
  await expect(run).not.toContainText(/\bcommand\b|\bdescription\b/);
  // The group head is one line, its 28 px chip included (once the project list is in: until then it warns it cannot check paths).
  await expect(approvals.getByTestId("approval-unknown-project")).toHaveCount(0);
  expect((await approvals.locator(".who").boundingBox())?.height).toBeLessThan(36);
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
  // Over a modal, the banner says what waits from under the title bar, not over its search.
  const search = await page.getByRole("button", { name: /Search or run/ }).boundingBox();
  await page.keyboard.press("Control+k");
  const banner = page.getByTestId("approvals-banner");
  await expect(banner).toBeVisible();
  expect((await banner.boundingBox())?.y).toBeGreaterThanOrEqual((search?.y ?? 0) + (search?.height ?? 0));
  await page.keyboard.press("Escape");
  await expect(banner).toBeHidden();
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
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Tidy up");
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

  await session.getByRole("textbox", { name: "Reply" }).fill("Check the status too");
  await session.getByRole("button", { name: "Send" }).click();
  const status = approvals.getByRole("group", { name: "Run git status" });
  await expect(status).toBeVisible();
  await expect(session.getByRole("status")).toHaveText("Needs approval");

  // Stopping denies what the agent still waits for.
  await session.getByRole("button", { name: "End task" }).click();
  await expect(session.getByRole("status")).toHaveText("Stopped");
  await expect(status).toBeHidden();
  await expect(session.getByRole("textbox", { name: "Reply" })).toBeHidden();
  // The list says so too; Close takes it off.
  await session.getByRole("button", { name: "Tasks" }).click();
  const agents = page.getByTestId("agents-panel");
  await expect(agents.getByRole("button", { name: /Tidy up.*Stopped/ })).toBeVisible();
  await agents.getByRole("button", { name: "Close Tidy up" }).click();
  await expect(agents.getByTestId("agents-empty")).toBeVisible();

  // The audit log says what was decided and by whom, newest first.
  await page.getByTestId("more").click();
  await page.getByRole("menuitem", { name: "Audit log" }).click();
  const audit = page.getByRole("region", { name: "Audit log" });
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

test("an agent's answer ends with a three-line summary", async ({ page }) => {
  await openProject(page, "ovra-agents-summary-");
  const agents = page.getByTestId("agents-panel");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Write a README.md for this project");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  const summary = agents.getByRole("definition");
  await expect(summary).toHaveText(["wrote README.md", "every command in it ran", "Nothing"]);
});

test("an answer's numbered items each start their own agent in one press", async ({ page }) => {
  await openGitProject(page, "ovra-agents-items-");
  const agents = page.getByTestId("agents-panel");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Find bugs");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  const items = agents.getByTestId("agent-items");
  await items.getByRole("checkbox", { name: /Race in watch\.rs/ }).check();
  await items.getByRole("checkbox", { name: /Typo in README\.md/ }).check();
  await items.getByRole("button", { name: "Do 1 and 2" }).click();
  await expect(items.getByTestId("agent-items-line")).toContainText("Started items 1 and 2");
  await agents.getByRole("button", { name: "Tasks" }).click();
  await expect(agents.locator(".row-button", { hasText: "Item" })).toHaveCount(2);
});

// The phase 3 gate (3.E): a task, three workers, three diffs in Reviews; keep
// one, discard one, comment on one.
test("a task's three workers wait in Reviews to be kept, discarded or sent back", async ({ page }) => {
  // Workers need a git repository for their worktrees.
  const dir = await openGitProject(page, "ovra-agents-task-");

  await page.getByRole("textbox", { name: "Describe a task" }).fill("Write the notes");
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
  await expect(page.getByTestId("toast").filter({ hasText: "approval" })).toHaveCount(0);
  for (let allowed = 0; allowed < 3; allowed++) {
    const card = approvals.getByRole("group", { name: "Write NOTES.md" }).first();
    await expect(card).toBeVisible();
    // The workers' requests arrive one by one, and each arrival restarts the wait.
    await clickWhenArmed(page, card.getByRole("button", { name: "Allow" }));
  }
  await expect(approvals).toContainText("Nothing waits for your approval.");

  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByTestId("inbox");
  const items = inbox.getByRole("navigation", { name: "Review items" }).getByRole("button");
  await expect(items).toHaveCount(3);
  for (const index of [0, 1, 2]) {
    await items.nth(index).click();
    const item = inbox.getByTestId("inbox-item");
    await expect(item).toContainText("Agent's summary: Wrote NOTES.md.");
    await expect(item.getByRole("list", { name: "Changed files" })).toContainText("NOTES.md");
    await expect(item.getByTestId("inbox-diff")).toContainText("Notes from a worker.");
  }

  const item = inbox.getByTestId("inbox-item");
  const pick = async (worker: string) => {
    await items.filter({ hasText: worker }).click();
    await expect(item.getByRole("heading", { level: 3 })).toContainText(worker);
  };
  // Keep the docs worker's notes: they land in the main checkout.
  await pick("docs");
  await item.getByRole("button", { name: "Keep changes" }).click();
  await expect.poll(() => readIfThere(join(dir, "NOTES.md"))).toBe("Notes from a worker.\n");

  // Discard the tests worker's notes, after the confirmation.
  await pick("tests");
  await item.getByRole("button", { name: "Discard…" }).click();
  await item.getByRole("button", { name: "Discard the changes" }).click();
  // A decision moves on to the next open item; the decided ones say how.
  await pick("docs");
  await expect(item.locator(".state")).toHaveText("Kept");
  await pick("tests");
  await expect(item.locator(".state")).toHaveText("Discarded");
  await expect.poll(() => readIfThere(join(dir, "NOTES.md"))).toBe("Notes from a worker.\n");

  // Send the lint worker back with a comment; its rework shows in the same
  // review once the person allows the write.
  await pick("lint");
  await item.getByRole("textbox", { name: "Comment to the agent" }).fill("Say what lint checks.");
  await item.getByRole("button", { name: /^Send comment/ }).click();
  await expect(item.getByRole("textbox", { name: "Comment to the agent" })).toHaveValue("");
  const rework = approvals.getByRole("group", { name: "Write NOTES.md" });
  await expect(rework).toBeVisible();
  await arm(page);
  await rework.getByRole("button", { name: "Allow" }).click();
  await expect(item).toContainText("Agent's summary: Reworked NOTES.md.");
  await expect(item.getByTestId("inbox-diff")).toContainText("Notes reworked after your comment.");
  await expect(item.locator(".state")).toHaveText("Waiting for review");

  // Discard the one part of the new notes in the worker's worktree, then put it back.
  const diff = item.getByTestId("inbox-diff");
  await diff.getByRole("button", { name: /^Discard part 1/ }).click();
  await expect(diff).toContainText("This review has no files.");
  await diff.getByRole("button", { name: "Put back the last discarded part" }).click();
  await expect(diff).toContainText("Notes reworked after your comment.");
});

// One agent in its own copy: no coordinator, one worktree, kept from Reviews.
test("one agent works on its own copy and its change is kept from Reviews, all from the keyboard", async ({ page }) => {
  const dir = await openGitProject(page, "ovra-agents-own-");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Write the notes");
  // The mode menu: Tab to it, open it with Enter, pick with the arrows.
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("agent-mode")).toBeFocused();
  await page.keyboard.press("Enter");
  const own = page.getByRole("menuitemradio", { name: /^One agent, own copy/ });
  await expect(page.getByRole("menuitemradio", { checked: true })).toBeFocused();
  await expectAccessible(page, "with the mode menu open");
  await expect(async () => {
    await page.keyboard.press("ArrowDown");
    await expect(own).toBeFocused({ timeout: 100 });
  }).toPass({ intervals: [0] });
  await page.keyboard.press("Enter");
  await expect(page.locator("#agent-mode-hint")).toContainText("Works on its own copy");
  // The choice gives focus back to the menu button; Start is further on.
  await expect(page.getByTestId("agent-mode")).toBeFocused();
  await tabTo(page, page.getByRole("button", { name: "Start", exact: true }));
  await page.keyboard.press("Enter");

  const approvals = page.getByTestId("approvals");
  const card = approvals.getByRole("group", { name: "Write NOTES.md" });
  await expect(card).toBeVisible();
  await page.keyboard.press("Alt+Shift+A");
  await expect(card).toBeFocused();
  // Its setup's output can arrive meanwhile and restart the wait before Y
  // counts: go to the card and press Y again until it is answered.
  await expect(async () => {
    await page.keyboard.press("Alt+Shift+A");
    await arm(page);
    await page.keyboard.press("y");
    await expect(approvals).toContainText("Nothing waits for your approval.", { timeout: 200 });
  }).toPass({ intervals: [0] });
  // It wrote its own copy, not the project's folder.
  expect(readIfThere(join(dir, "NOTES.md"))).toBeNull();

  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByTestId("inbox");
  await expect(inbox.getByRole("navigation", { name: "Review items" }).getByRole("button")).toHaveCount(1);
  // Focus goes into Reviews: on its heading, or on the item when it was already listed.
  await expect.poll(() => inbox.evaluate((box) => box.contains(document.activeElement))).toBe(true);
  const item = inbox.getByTestId("inbox-item");
  await expect(item.getByTestId("inbox-diff")).toContainText("Notes from a worker.");
  // "Explain these changes": a read-only agent's plain words, above the file's diff.
  await tabTo(page, item.getByTestId("inbox-explain"));
  await page.keyboard.press("Enter");
  await expect(item.getByTestId("inbox-file-explained")).toHaveText("Adds a line about the agent's work.");
  await expectAccessible(page, "in Reviews with an item and its explanation open");
  // Keep sits above the explanation, a few Shift+Tab presses back, and Enter presses it.
  const keep = item.getByRole("button", { name: "Keep changes" });
  await tabTo(page, keep, { back: true });
  // The item can reload as the agent's turn ends; Keep waits for that, as a person would.
  await expect(keep).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("Enter");
  await expect.poll(() => readIfThere(join(dir, "NOTES.md"))).toBe("Notes from a worker.\n");
});

// The daemon's fake forge has the project's own branches on `origin`, and
// gives each pull request three checks: build passed, test failed, lint runs.
test("a worker's changes open as a draft pull request whose checks show on the item", async ({ page }) => {
  await openGitProject(page, "ovra-agents-pr-");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Write the notes");
  await page.getByTestId("agent-mode").click();
  await page.getByRole("menuitemradio", { name: /^Plan and split/ }).click();
  await page.getByRole("button", { name: "Start", exact: true }).click();
  const approvals = page.getByTestId("approvals");
  for (let allowed = 0; allowed < 3; allowed++) {
    const card = approvals.getByRole("group", { name: "Write NOTES.md" }).first();
    await expect(card).toBeVisible();
    await clickWhenArmed(page, card.getByRole("button", { name: "Allow" }));
  }
  await expect(approvals).toContainText("Nothing waits for your approval.");

  await page.keyboard.press("Alt+Shift+R");
  const inbox = page.getByTestId("inbox");
  const item = inbox.getByTestId("inbox-item");
  await expect(item).toContainText("Agent's summary: Wrote NOTES.md.");
  await item.getByRole("button", { name: "Open pull request…" }).click();

  const dialog = page.getByRole("dialog", { name: "Open pull request" });
  await expect(dialog.getByRole("list", { name: "Files in the pull request" })).toContainText("NOTES.md");
  await expect(dialog.getByTestId("pr-where")).toContainText("Signed in as octo on github.com");
  await expect(dialog.getByRole("textbox", { name: "Base branch" })).toHaveValue("main");
  await expect(dialog.getByRole("textbox", { name: "Title" })).toHaveValue("Wrote NOTES.md.");
  await expect(dialog.getByRole("checkbox", { name: "Draft" })).toBeChecked();
  await dialog.getByRole("textbox", { name: "Title" }).fill("Add the notes");
  await dialog.getByRole("button", { name: "Create pull request" }).click();
  await expect(dialog).toBeHidden();

  await expect(item.getByTestId("pr-chip")).toHaveText(/^PR #\d+ · draft · 1 of 3 checks passed$/);
  await expect(item.locator(".state").first()).toHaveText("Pull request opened");
  const checks = item.getByRole("list", { name: "Checks" });
  await expect(checks.getByRole("listitem")).toHaveText([/Passed\s*build/, /Failed\s*test/, /Running\s*lint/]);
  await expect(checks.getByRole("button", { name: "Send the log of test to the worker" })).toBeVisible();
  // Keep and Discard stay for work that should land locally too.
  await expect(item.getByRole("button", { name: "Keep changes", exact: true })).toBeVisible();
});

test("a direct session's protected files and git hooks need a look before approve", async ({ page }) => {
  const dir = await openGitProject(page, "ovra-agents-protected-");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Fix the typo");
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
  const inbox = page.getByTestId("inbox");
  const entry = inbox.getByRole("navigation", { name: "Review items" }).getByRole("button");
  await expect(entry).toHaveCount(1);
  await expect(entry.getByTestId("item-protected")).toHaveText("2 files can run code or change agent rules");
  await expect(entry.getByTestId("item-git")).toHaveText("1 git hook or config file changed");
  await entry.click();
  const item = inbox.getByTestId("inbox-item");
  await expect(item.getByTestId("inbox-git")).toContainText("pre-commit (added)");
  await expect(item.getByTestId("inbox-git")).toContainText("leave them as they are");
  await expect(item.getByTestId("inbox-protected")).toContainText("Discard leaves .envrc, .mcp.json as they are");
  await expect(item.getByTestId("inbox-protected")).not.toContainText("Discard restores");

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

test("always allow adds a rule, and the next identical request is allowed by it", async ({ page }) => {
  // Tasks that start with "Sandboxed" run `npm test` twice in a fake sandbox with a limited network.
  await openProject(page, "ovra-agents-always-");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Sandboxed: run the tests");
  await page.keyboard.press("ControlOrMeta+Enter");
  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run npm test" });
  await expect(run).toBeVisible();
  const always = run.getByTestId("approval-always");
  await expect(always).toHaveText("Always allow npm test here");
  await arm(page);
  await expect(always).toHaveAttribute("aria-disabled", "false");
  await always.click();

  // The second `npm test` asks nobody: the turn ends with both runs.
  const session = page.getByTestId("agent-session");
  await expect(session.getByRole("status")).toHaveText("Done");
  await expect(approvals).toContainText("Nothing waits for your approval.");
  const transcript = page.getByTestId("transcript");
  await expect(transcript).toContainText("The tests ran twice.");
  await expect(transcript.locator(".step summary", { hasText: "Ran npm test" })).toHaveCount(2);

  // The rule shows in Settings > Rules, exact and marked, with Remove.
  await page.keyboard.press("ControlOrMeta+K");
  await page.keyboard.type(">rules");
  await page.getByRole("option", { name: /Rules/ }).click();
  const rule = page.getByTestId("policy-rule");
  await expect(rule).toHaveCount(1);
  await expect(rule).toContainText(/Bash\(npm test\)\s*exactly\s*Auto\s*From an approval/);
  await expect(rule.getByRole("button", { name: "Remove Bash(npm test) exactly auto" })).toBeVisible();
});

test("a research agent changes nothing, its report is saved as a doc, and a schedule runs one", async ({ page }) => {
  const dir = await openGitProject(page, "ovra-agents-research-");
  await page.getByTestId("agent-mode").click();
  await page.getByRole("menuitemradio", { name: /^Research/ }).click();
  await page.getByRole("textbox", { name: "Describe a task" }).fill("How does the readme read?");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  // The fake agent asks to run `ls -la` and edit README.md: research refuses
  // both without asking, so nothing waits for an answer.
  const save = page.getByRole("button", { name: "Save as doc" });
  await expect(save).toBeVisible();
  await save.click();
  const day = new Date().toISOString().slice(0, 10);
  const path = `docs/research/${day}-how-does-the-readme-read.md`;
  await expect(page.getByTestId("agent-report")).toContainText(`Saved as ${path}`);
  expect(readIfThere(join(dir, path))).toContain("# How does the readme read?");
  expect(readFileSync(join(dir, "README.md"), "utf8")).toBe("teh readme\n");

  // A schedule, once switched on in Settings, runs the same kind of agent.
  await page.keyboard.press("ControlOrMeta+Comma");
  const settings = page.getByRole("dialog", { name: "Settings" });
  const panes = settings.getByRole("navigation", { name: "Settings panes" });
  await panes.getByRole("button", { name: "Agents" }).click();
  // Schedules are an extra feature, folded away until asked for.
  await settings.getByText("Extra features", { exact: true }).click();
  // Off by default, and no Schedules pane until it is on.
  const toggle = settings.getByTestId("schedules-switch");
  await expect(toggle).not.toBeChecked();
  await expect(panes.getByRole("button", { name: "Schedules" })).toHaveCount(0);
  await toggle.check();
  try {
    await settings.getByRole("button", { name: "Set up schedules" }).click();
    const pane = settings.getByTestId("schedules-pane");
    await pane.getByRole("button", { name: "Add schedule" }).click();
    await pane.getByLabel("Name").fill("Nightly check");
    await pane.getByLabel("What to do").fill("How does the readme read?");
    // Twelve hours away, so the clock leaves it alone during the test.
    const hour = String((new Date().getHours() + 12) % 24).padStart(2, "0");
    await pane.getByLabel("Time").fill(`${hour}:00`);
    await pane.getByRole("button", { name: "Add schedule" }).click();
    const row = pane.getByTestId("schedule-row");
    await expect(row).toContainText("Every day at");
    await expect(row.getByTestId("schedule-next")).toContainText("Next run");
    await row.getByRole("button", { name: "Run now" }).click();
    await expect(row.getByTestId("schedule-last")).toContainText("Last run started");
    await page.keyboard.press("ControlOrMeta+Comma");
    // The research report is still open: back to the list of tasks.
    await page.getByTestId("agents").getByRole("button", { name: "Tasks" }).click();
    await expect(page.getByTestId("agents").getByText("Scheduled · Nightly check")).toBeVisible();
  } finally {
    // The daemon is shared: other tests see Settings as they expect.
    await page.keyboard.press("ControlOrMeta+Comma");
    await panes.getByRole("button", { name: "Agents" }).click();
    await settings.getByTestId("schedules-switch").uncheck();
  }
});

test("add a file with @ and send it with the task", async ({ page }) => {
  await openProject(page, "ovra-agents-context-");
  const field = page.getByRole("textbox", { name: "Describe a task" });
  await field.fill("Fix the typo in ");
  await field.press("End");
  await page.keyboard.type("@READ");
  const option = page.getByTestId("context-menu").getByRole("option", { name: /README\.md/ });
  await expect(option.first()).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(field).toHaveValue("Fix the typo in README.md ");
  await expect(page.getByTestId("context-chip")).toHaveText(/README\.md/);
  // `@#` lists GitHub issues through the fake forge; the chosen one goes too.
  await page.keyboard.type("and @#login");
  await expect(page.getByTestId("context-menu").getByRole("option", { name: /#12/ })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("context-chip")).toHaveText([/README\.md/, /#12/]);
  await page.keyboard.press("Enter");

  // The agent gets the file's name after the task; the transcript folds it.
  const session = page.getByTestId("agent-session");
  const sent = session.locator("details", { hasText: "Context sent" });
  await expect(sent).toBeVisible();
  await sent.locator("summary").click();
  await expect(sent).toContainText("File: `README.md`");
  await expect(sent).toContainText("GitHub issue #12");
  await session.getByRole("button", { name: "End task" }).click();
  await expect(session.getByRole("status")).toHaveText("Stopped");
});

test("keep the computer awake while an agent works", async ({ page }) => {
  await openProject(page, "ovra-agents-awake-");
  const awake = page.getByTestId("keep-awake");
  const held = /(^|\s)awake(\s|$)/;
  await expect(awake).toHaveAttribute("aria-pressed", "false");
  await awake.click();
  await expect(awake).toHaveAttribute("aria-pressed", "true");
  await expect(awake).not.toHaveClass(held);
  // The fake daemon holds a fake: nothing keeps the test machine awake.
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(page.getByTestId("approvals").getByRole("group", { name: "Run ls -la" })).toBeVisible();
  await expect(awake).toHaveClass(held);
  await expect(page.getByTestId("keep-awake-tip")).toContainText("Keep awake: on");
  await expect(page.getByTestId("keep-awake-tip")).toContainText("Keeping the computer awake");

  const session = page.getByTestId("agent-session");
  await session.getByRole("button", { name: "End task" }).click();
  await expect(session.getByRole("status")).toHaveText("Stopped");
  await expect(awake).not.toHaveClass(held);
  await expect(awake).toHaveAttribute("aria-pressed", "true");
  await awake.click();
  await expect(awake).toHaveAttribute("aria-pressed", "false");
});

test("Focus shows only the agents' work and answers approvals there", async ({ page }) => {
  // Its own daemon: Focus shows the approvals of every project, and agents
  // of other files' tests ask on the shared one meanwhile.
  const own = await startDaemon(["--fake-agent"]);
  try {
    await focusFlow(page, own.url);
    await stopAgents(page, own.url);
    // The shared daemon's afterEach has nothing of this project to stop.
    await page.evaluate(() => localStorage.removeItem("ovra.project"));
  } finally {
    own.stop();
  }
});

async function focusFlow(page: Page, url: string): Promise<void> {
  await openProject(page, "ovra-agents-focus-", url);
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");
  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();

  // Found by name in the title bar.
  await page.getByTestId("focus-switch").click();
  await expect(page.getByTestId("focus-bar")).toBeVisible();
  // What had focus is hidden: it moves to the way back.
  await expect(page.getByTestId("focus-open-ide")).toBeFocused();
  // No code, tree or terminal: only the agents' work.
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeHidden();
  await expect(page.getByTestId("focus-progress")).toContainText("Fix the typo");
  await expect(run).toBeVisible();
  // The card is on screen: no toast repeats it.
  await expect(page.getByTestId("toast").filter({ hasText: "approval" })).toHaveCount(0);
  await expect(approvals.getByTestId("approval-always")).toHaveCount(0);
  await expectAccessible(page, "in Focus");

  await page.keyboard.press("Alt+Shift+A");
  await expect(run).toBeFocused();
  await arm(page);
  await page.keyboard.press("y");
  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeVisible();
  await page.keyboard.press("Alt+Shift+A");
  await arm(page);
  await page.keyboard.press("n");
  await expect(page.getByTestId("focus-nothing")).toHaveText("Nothing waits for you.");

  await page.keyboard.press("ControlOrMeta+Shift+Period");
  await expect(page.getByTestId("focus-bar")).toHaveCount(0);
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
}

test("go back to a message: its file is gone, the message is back in the box, and undo brings it back", async ({
  page,
}) => {
  // Tasks that start with "Checkpoint" write one.txt, then two.txt on the reply.
  const dir = await openGitProject(page, "ovra-agents-checkpoints-");
  const approvals = page.getByTestId("approvals");
  const allow = async (name: string) => {
    const card = approvals.getByRole("group", { name });
    await expect(card).toBeVisible();
    await arm(page);
    const yes = card.getByRole("button", { name: "Allow" });
    await expect(yes).toHaveAttribute("aria-disabled", "false");
    await yes.click();
  };
  const session = page.getByTestId("agent-session");
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Checkpoint: write one");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await allow("Write one.txt");
  await expect(session.getByTestId("transcript").getByText("Wrote one.txt.", { exact: true })).toBeVisible();
  const reply = page.getByRole("textbox", { name: "Reply", exact: true });
  await reply.fill("Now write two");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await allow("Write two.txt");
  await expect(session.getByTestId("transcript").getByText("Wrote two.txt.", { exact: true })).toBeVisible();
  await expect(session.getByRole("status")).toHaveText("Done");
  expect(readIfThere(join(dir, "two.txt"))).toBe("two.txt from the agent\n");

  // From the keyboard: the quiet action shows on focus.
  const back = session.getByRole("button", { name: "Go back to here" });
  await expect(back).toHaveCount(2);
  await back.nth(1).focus();
  await page.keyboard.press("Enter");
  const confirm = session.getByTestId("go-back-confirm");
  await expect(confirm).toContainText("Undo the agent's changes from turn 2?");
  await confirm.getByRole("button", { name: "Go back", exact: true }).click();
  const result = session.getByTestId("go-back-result");
  await expect(result).toContainText("Set back: two.txt.");
  expect(readIfThere(join(dir, "two.txt"))).toBeNull();
  expect(readIfThere(join(dir, "one.txt"))).toBe("one.txt from the agent\n");
  await expect(reply).toHaveValue("Now write two");
  await expect(session.getByTestId("set-back-divider")).toBeVisible();

  await result.getByRole("button", { name: "Undo go back" }).click();
  await expect(result).toContainText("Put back: two.txt.");
  expect(readIfThere(join(dir, "two.txt"))).toBe("two.txt from the agent\n");
  await expect(session.getByTestId("set-back-divider")).toBeHidden();
});

// Worktree setup (docs/specs/worktree-setup.md): the fake dev servers run a
// setup step named `fail` as a failure, so each worker stays preparing with
// the failed step until the person starts it anyway.
test("a worker is prepared, its setup fails, and Start anyway starts it", async ({ page }) => {
  const setup = { setup: [{ command: ["pnpm", "install"] }, { command: ["fail"] }] };
  await openGitProject(page, "ovra-agents-setup-", { ".ovra/worktree.json": JSON.stringify(setup) });
  await page.getByRole("textbox", { name: "Describe a task" }).fill("Write the notes");
  await page.getByTestId("agent-mode").click();
  await page.getByRole("menuitemradio", { name: /^Plan and split/ }).click();
  await page.getByRole("button", { name: "Start", exact: true }).click();

  // The task opens on its conversation; its workers are in the list.
  const agents = page.getByTestId("agents");
  await agents.getByRole("button", { name: "Tasks" }).click();
  const docs = agents.locator(".row-button", { hasText: "docs" });
  await expect(docs.locator(".state")).toHaveText("Setup failed at 2 of 2: fail");
  const prepared = agents.getByTestId("worker-setup").first();
  await expect(prepared.getByTestId("worker-setup-failed")).toHaveText("Step 2 of 2: fail exited with code 1");
  await prepared.getByRole("button", { name: "Show output" }).click();
  await expect(prepared.getByTestId("worker-setup-log")).toHaveText("fake fail");
  await expect(prepared.getByRole("button", { name: "Retry" })).toBeVisible();

  await prepared.getByRole("button", { name: "Start anyway" }).click();
  await expect(agents.getByTestId("worker-setup")).toHaveCount(2);
  // The worker's agent starts in its worktree and asks to write its notes.
  const card = page.getByTestId("approvals").getByRole("group", { name: "Write NOTES.md" });
  await expect(card).toBeVisible();
  await arm(page);
  await card.getByRole("button", { name: "Allow" }).click();
  await expect(docs.locator(".state")).not.toHaveText(/Setup failed/);

  // Stopping the task stops the workers still waiting on their setup.
  await agents.locator(".row-button", { hasText: "Write the notes" }).first().click();
  await page.getByTestId("agent-session").getByRole("button", { name: "End task" }).click();
  await agents.getByRole("button", { name: "Tasks" }).click();
  await expect(docs).toBeVisible();
  await expect(agents.getByTestId("worker-setup")).toHaveCount(0);
});

test("History finds a past task of another project and opens it to continue", async ({ page }) => {
  await openProject(page, "ovra-agents-history-a-");
  const word = `zebra${Date.now()}`;
  const agents = page.getByTestId("agents-panel");
  await page.getByRole("textbox", { name: "Describe a task" }).fill(`Write a README.md about ${word}`);
  await page.keyboard.press("ControlOrMeta+Enter");
  await expect(agents.getByRole("definition")).toHaveText(["wrote README.md", "every command in it ran", "Nothing"]);

  // Another project is open now; History still has the task.
  const other = mkdtempSync(join(tmpdir(), "ovra-agents-history-b-"));
  const id = await addProject(process.env.OVRA_URL ?? "", other);
  await page.evaluate((project) => localStorage.setItem("ovra.project", project), id);
  await page.reload();
  await expect(page.getByRole("button", { name: `Project ${basename(other)}, switch project` })).toBeVisible();
  await agents.getByTestId("agents-history").click();
  const history = page.getByTestId("history");
  await history.getByTestId("history-search").fill(word.toUpperCase());
  const row = history.getByTestId("history-row");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText(`Write a README.md about ${word}`);
  await row.click();

  await expect(history).toBeHidden();
  await expect(agents.getByTestId("agent-session")).toContainText(`Write a README.md about ${word}`);
  await expect(agents.getByRole("definition")).toHaveText(["wrote README.md", "every command in it ran", "Nothing"]);
});
