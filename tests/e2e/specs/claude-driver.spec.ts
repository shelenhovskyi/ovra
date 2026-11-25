import { copyFileSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import type { Page } from "@playwright/test";
import { binary, root, startDaemon } from "../daemon";
import { expect, test } from "../test";

// The real claude driver against `fake-claude`, which plays a recorded
// session (crates/agent-runtime/fixtures/edit_allowed.jsonl). This file has
// its own daemon: the shared one runs the scripted fake agent instead.
test.describe.configure({ mode: "serial" });

let daemon: { url: string; stop: () => void } | undefined;

test.beforeAll(async () => {
  // Found first on PATH under the name Ovra looks for.
  const bin = mkdtempSync(join(tmpdir(), "ovra-fake-claude-"));
  copyFileSync(binary("fake-claude"), join(bin, process.platform === "win32" ? "claude.exe" : "claude"));
  daemon = await startDaemon([], {
    ...process.env,
    PATH: `${bin}${delimiter}${process.env.PATH ?? ""}`,
    OVRA_FAKE_CLAUDE_FIXTURE: resolve(root, "crates/agent-runtime/fixtures/edit_allowed.jsonl"),
  });
});

test.afterAll(() => daemon?.stop());

/** Approvals wait 500 ms before they can be answered; skip that wait. */
async function allow(page: Page, name: RegExp): Promise<void> {
  const card = page.getByTestId("approvals").getByRole("group", { name });
  await expect(card).toBeVisible();
  await page.clock.fastForward(600);
  await card.getByRole("button", { name: "Allow" }).click();
}

test("a recorded Claude session runs through the real driver", async ({ page }) => {
  if (!daemon) throw new Error("the daemon did not start");
  const dir = mkdtempSync(join(tmpdir(), "ovra-driver-"));
  writeFileSync(join(dir, "README.md"), "# Demo\n\nThis is teh demo.\n");
  await page.clock.install();
  await page.goto(daemon.url);
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();

  // The recording's prompts, word for word: fake-claude checks every line.
  await page
    .getByRole("textbox", { name: "What should Claude do?" })
    .fill("Fix the typo in README.md with the Edit tool. Do not run commands and do not change anything else.");
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await allow(page, /^Edit .*README\.md$/);

  const transcript = page.getByTestId("transcript");
  await expect(transcript).toContainText(/I fixed the typo in `?README\.md`? line 3/);
  const session = page.getByTestId("agent-session");
  await expect(session.getByRole("status")).toHaveText("Done");

  await session
    .getByRole("textbox", { name: "Reply to Claude" })
    .fill("Now run `echo ok > done.txt` with the Bash tool once, then say done.");
  await session.getByRole("button", { name: "Send" }).click();
  await allow(page, /^Run echo ok > done\.txt$/);
  await expect(transcript).toContainText(/Claude\s*Done\./);
  await expect(session.getByRole("status")).toHaveText("Done");

  await session.getByRole("button", { name: "Stop" }).click();
  // A mismatch with the recording would make fake-claude exit 2: Failed.
  await expect(session.getByRole("status")).toHaveText("Stopped");
});
