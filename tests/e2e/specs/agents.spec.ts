import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, type Page, test } from "@playwright/test";

// The daemon runs with --fake-agent: every session asks to run `ls -la` and to
// edit README.md, then waits; each reply asks to run `git status`.
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

test("start an agent and answer its requests from the keyboard", async ({ page }) => {
  await openProject(page, "ovra-agents-keys-");
  await page.getByRole("textbox", { name: "Task for a new Claude session" }).fill("Fix the typo");
  await page.keyboard.press("ControlOrMeta+Enter");

  const approvals = page.getByTestId("approvals");
  const run = approvals.getByRole("group", { name: "Run ls -la" });
  await expect(run).toBeVisible();
  await expect(page.getByTestId("notifications")).toContainText("Fix the typo needs approval: Run ls -la");
  // Nothing moves focus to the request: the prompt field keeps it.
  await expect(page.getByRole("textbox", { name: "Task for a new Claude session" })).toBeFocused();

  await page.keyboard.press("Alt+Shift+A");
  await expect(run).toBeFocused();
  await arm(page);
  await expect(run.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("y");

  const edit = approvals.getByRole("group", { name: "Edit README.md" });
  await expect(edit).toBeFocused();
  await arm(page);
  await expect(edit.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await page.keyboard.press("Shift+N");
  await expect(edit.getByRole("textbox", { name: "Message to the agent" })).toBeFocused();
  await page.keyboard.type("Leave the readme alone");
  await page.keyboard.press("ControlOrMeta+Enter");

  const session = page.getByTestId("agent-session");
  await expect(session.getByRole("status")).toHaveText("Waiting for you");
  await expect(approvals).toContainText("Nothing waits for your approval.");
  const transcript = page.getByTestId("transcript");
  await expect(transcript).toContainText(/Tool result\s*done/);
  await expect(transcript).toContainText(/Tool failed\s*Leave the readme alone/);
  await expect(transcript).toContainText(/Claude\s*Done\./);
});

test("reply to an agent, allow with the mouse and stop it", async ({ page }) => {
  await openProject(page, "ovra-agents-reply-");
  await page.getByRole("textbox", { name: "Task for a new Claude session" }).fill("Tidy up");
  await page.getByRole("button", { name: "Start" }).click();

  const approvals = page.getByTestId("approvals");
  for (const name of ["Run ls -la", "Edit README.md"]) {
    const card = approvals.getByRole("group", { name });
    await expect(card).toBeVisible();
    await arm(page);
    await expect(card.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
    await card.getByRole("button", { name: "Allow" }).click();
  }
  const session = page.getByTestId("agent-session");
  await expect(session.getByRole("status")).toHaveText("Waiting for you");

  await session.getByRole("textbox", { name: "Reply to Claude" }).fill("Check the status too");
  await session.getByRole("button", { name: "Send" }).click();
  const status = approvals.getByRole("group", { name: "Run git status" });
  await expect(status).toBeVisible();
  await expect(page.getByRole("button", { name: /Tidy up.*Needs approval/ })).toBeVisible();
  await arm(page);
  await expect(status.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await status.getByRole("button", { name: "Allow" }).click();
  await expect(session.getByRole("status")).toHaveText("Waiting for you");

  await session.getByRole("button", { name: "Stop" }).click();
  await expect(session.getByRole("status")).toHaveText("Stopped");
  await expect(session.getByRole("textbox", { name: "Reply to Claude" })).toBeHidden();
});
