import { copyFileSync, cpSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import type { Page } from "@playwright/test";
import { binary, root, startDaemon } from "../daemon";
import { expect, loadProject, test } from "../test";

// The real claude driver against `fake-claude`, which plays the recorded
// session (crates/agent-runtime/fixtures) whose prompt each test sends: the
// contract tests replay the same files. This file has its own daemon: the
// shared one runs the scripted fake agent instead.
test.describe.configure({ mode: "serial" });

/** Where each project keeps a copy of the recordings. fake-claude runs in
 * the sandbox, which hides the home folder (and so a checkout in it), but
 * shows the work dir: a relative fixture path is read from there. */
const fixtures = "fake-claude-fixtures";

let daemon: { url: string; stop: () => void } | undefined;

test.beforeAll(async () => {
  // Found first on PATH under the name Ovra looks for.
  const bin = mkdtempSync(join(tmpdir(), "ovra-fake-claude-"));
  copyFileSync(binary("fake-claude"), join(bin, process.platform === "win32" ? "claude.exe" : "claude"));
  daemon = await startDaemon([], {
    ...process.env,
    PATH: `${bin}${delimiter}${process.env.PATH ?? ""}`,
    OVRA_FAKE_CLAUDE_FIXTURE: fixtures,
  });
});

test.afterAll(() => daemon?.stop());

/** Approvals wait 500 ms before they can be answered; skip that wait. */
async function allow(page: Page, name: RegExp): Promise<void> {
  const card = page.getByTestId("approvals").getByRole("group", { name });
  await expect(card).toBeVisible();
  await page.clock.fastForward(600);
  await card.getByRole("button", { name: "Allow once" }).click();
}

/** Opens a new project with a README and starts Claude on `prompt`. */
async function start(page: Page, prompt: string): Promise<void> {
  if (!daemon) throw new Error("the daemon did not start");
  const dir = mkdtempSync(join(tmpdir(), "ovra-driver-"));
  writeFileSync(join(dir, "README.md"), "# Demo\n\nThis is teh demo.\n");
  cpSync(resolve(root, "crates/agent-runtime/fixtures"), join(dir, fixtures), { recursive: true });
  await page.clock.install();
  await loadProject(page, dir, daemon.url);
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  // The recording's prompts, word for word: fake-claude checks every line.
  await page.getByRole("textbox", { name: "Describe a task" }).fill(prompt);
  await page.getByRole("button", { name: "Start", exact: true }).click();
}

/** Stops the session; a mismatch with the recording would have made fake-claude exit 2: Failed. */
async function stop(page: Page): Promise<void> {
  const session = page.getByTestId("agent-session");
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("combobox", { name: "Type a command" }).fill("Stop the agent");
  await page.keyboard.press("Enter");
  await expect(session.getByRole("status")).toHaveText("Stopped");
}

test("a recorded Claude session runs through the real driver", async ({ page }) => {
  await start(
    page,
    "Fix the typo in README.md with the Edit tool. Do not run commands and do not change anything else.",
  );
  await allow(page, /^Edit .*README\.md$/);

  const transcript = page.getByTestId("transcript");
  await expect(transcript).toContainText(/I fixed the typo in `?README\.md`? line 3/);
  const session = page.getByTestId("agent-session");
  await expect(session.getByRole("status")).toHaveText("Done");

  await session
    .getByRole("textbox", { name: "Reply" })
    .fill("Now run `echo ok > done.txt` with the Bash tool once, then say done.");
  await session.getByRole("button", { name: "Send" }).click();
  await allow(page, /^Run echo ok > done\.txt$/);
  await expect(transcript).toContainText(/Claude\s*Done\./);
  await expect(session.getByRole("status")).toHaveText("Done");

  // A Done session shows no Stop button; the palette still stops it.
  await expect(session.getByRole("button", { name: "Stop" })).toHaveCount(0);
  await stop(page);
});

test("an interrupted turn says the person stopped it, and the session goes on", async ({ page }) => {
  await start(
    page,
    "Run the shell command `ping -c 120 127.0.0.1` with the Bash tool in the foreground (not in the background), then tell me its last line.",
  );
  await allow(page, /^Run ping -c 120 127\.0\.0\.1$/);
  const session = page.getByTestId("agent-session");
  await session.getByRole("button", { name: "Interrupt" }).click();

  const transcript = page.getByTestId("transcript");
  await expect(transcript).toContainText("You interrupted this turn.");
  await expect(transcript.locator("li.error")).toHaveCount(0);
  await expect(transcript.getByRole("button", { name: "Retry" })).toHaveCount(0);
  await expect(session.getByRole("status")).toHaveText("Done");

  await session.getByRole("textbox", { name: "Reply" }).fill("Never mind. Reply with just the word: ready");
  await session.getByRole("button", { name: "Send" }).click();
  await expect(transcript).toContainText(/Claude\s*ready/);
  await stop(page);
});
