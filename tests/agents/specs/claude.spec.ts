import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { expect, type Locator, type Page, test } from "@playwright/test";
import { toFixture } from "../fixtures.ts";

// The real `claude` CLI through Ovra: the Agents panel starts a session, its
// tool calls come to Approvals, and each session is recorded. With
// OVRA_UPDATE_FIXTURES=1 the recordings replace the contract test fixtures
// in crates/agent-runtime/fixtures.
test.describe.configure({ mode: "serial" });

const fixtures = resolve(dirname(fileURLToPath(import.meta.url)), "../../../crates/agent-runtime/fixtures");

function env(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set; globalSetup did not run`);
  return value;
}

/** A project `<live>/<name>/w`; returns the folder holding it. */
function makeProject(name: string, files: Record<string, string>): { base: string; dir: string } {
  const base = join(env("OVRA_LIVE_DIR"), name);
  const dir = join(base, "w");
  for (const [path, text] of Object.entries(files)) {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    writeFileSync(join(dir, path), text);
  }
  return { base, dir };
}

async function open(page: Page, dir: string): Promise<void> {
  await page.goto(env("OVRA_URL"));
  const projects = page.getByTestId("project-list");
  await projects.getByRole("textbox", { name: "Open a folder" }).fill(dir);
  await projects.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("treeitem", { name: /README\.md/ })).toBeVisible();
  await expect(page.getByTestId("claude-setup")).toContainText("is ready");
}

async function startSession(page: Page, prompt: string): Promise<Locator> {
  await page.getByRole("textbox", { name: "What should Claude do?" }).fill(prompt);
  await page.getByRole("button", { name: "Start", exact: true }).click();
  return page.getByTestId("agent-session");
}

/** Answers the approval card named `name` once it can be answered. */
async function answer(page: Page, name: RegExp, decision: "Allow" | "Deny"): Promise<void> {
  const card = page.getByTestId("approvals").getByRole("group", { name });
  await expect(card).toBeVisible();
  const button = card.getByRole("button", { name: decision, exact: true });
  await expect(button).toHaveAttribute("aria-disabled", "false");
  await button.click();
  await expect(card).toBeHidden();
}

async function denyWithMessage(page: Page, name: RegExp, message: string): Promise<void> {
  const card = page.getByTestId("approvals").getByRole("group", { name });
  await expect(card).toBeVisible();
  await expect(card.getByRole("button", { name: "Allow" })).toHaveAttribute("aria-disabled", "false");
  await card.getByRole("button", { name: "Deny with message…" }).click();
  await card.getByRole("textbox", { name: "Message to the agent" }).fill(message);
  const send = card.getByRole("button", { name: "Send denial" });
  await expect(send).toHaveAttribute("aria-disabled", "false");
  await send.click();
  await expect(card).toBeHidden();
}

async function waitingForYou(session: Locator): Promise<void> {
  await expect(session.getByRole("status")).toHaveText("Waiting for you");
}

async function reply(session: Locator, text: string): Promise<void> {
  await session.getByRole("textbox", { name: "Reply to Claude" }).fill(text);
  await session.getByRole("button", { name: "Send" }).click();
}

async function stop(session: Locator): Promise<void> {
  await session.getByRole("button", { name: "Stop" }).click();
  await expect(session.getByRole("status")).toHaveText("Stopped");
}

/** The one recording a test made: its lines, and the fixture when asked for. */
function recording(before: Set<string>, base: string, fixture: string): Record<string, unknown>[] {
  const dir = env("OVRA_RECORD_DIR");
  const made = readdirSync(dir).filter((f) => !before.has(f));
  expect(made).toHaveLength(1);
  const text = readFileSync(join(dir, made[0] as string), "utf8");
  if (process.env.OVRA_UPDATE_FIXTURES === "1") {
    writeFileSync(join(fixtures, `${fixture}.jsonl`), toFixture(text, base));
  }
  return text
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l));
}

function recorded(): Set<string> {
  return new Set(readdirSync(env("OVRA_RECORD_DIR")));
}

type Line = Record<string, unknown> & { request?: Record<string, unknown> };
const cliLines = (lines: Record<string, unknown>[]): Line[] =>
  lines.flatMap((l) => (l.cli && typeof l.cli === "object" ? [l.cli as Line] : []));

let editSession: { dir: string; id: string } | undefined;

test("allow an edit, reply, allow a command and stop", async ({ page }) => {
  const { base, dir } = makeProject("edit", { "README.md": "# Demo\n\nThis is teh demo.\n" });
  const before = recorded();
  await open(page, dir);
  const session = await startSession(
    page,
    "Fix the typo in README.md with the Edit tool. Do not run commands and do not change anything else.",
  );
  await answer(page, /Edit .*README\.md/, "Allow");
  await waitingForYou(session);
  expect(readFileSync(join(dir, "README.md"), "utf8")).toBe("# Demo\n\nThis is the demo.\n");

  await reply(session, "Now run `echo ok > done.txt` with the Bash tool once, then say done.");
  await answer(page, /Run echo ok > done\.txt/, "Allow");
  await waitingForYou(session);
  expect(readFileSync(join(dir, "done.txt"), "utf8")).toBe("ok\n");
  await expect(page.getByTestId("transcript")).toContainText("Tool output");
  await stop(session);

  const lines = cliLines(recording(before, base, "edit_allowed"));
  const init = lines.find((l) => l.type === "system" && l.subtype === "init");
  expect(init?.permissionMode).toBe("default");
  editSession = { dir, id: String(init?.session_id) };
});

test("a hook asks before a write outside the project even when settings allow it", async ({ page }) => {
  const base = join(env("OVRA_LIVE_DIR"), "outside");
  // The project's own settings would allow the write without asking.
  const { dir } = makeProject("outside", {
    "README.md": "# Notes\n",
    ".claude/settings.json": JSON.stringify({ permissions: { allow: ["Write", `Edit(/${base}/**)`] } }),
  });
  const before = recorded();
  await open(page, dir);
  const trust = page.getByTestId("project-trust");
  await expect(trust.getByTestId("broad-rules")).toContainText("without asking you");
  await trust.getByRole("button", { name: "Trust project" }).click();
  await expect(trust.getByRole("heading")).toHaveText("Project settings are on");

  const session = await startSession(
    page,
    "Use the Write tool to create the file ../notes.txt (next to this project folder, not inside it) " +
      "containing the word hi. If the write is denied, do not retry; just say so.",
  );
  await denyWithMessage(page, /notes\.txt/, "Keep files inside the project.");
  await waitingForYou(session);
  await expect(page.getByTestId("transcript")).toContainText("Keep files inside the project.");
  expect(existsSync(join(base, "notes.txt"))).toBe(false);
  await stop(session);

  const lines = cliLines(recording(before, base, "denied_outside_folder"));
  const asks = lines.filter((l) => l.type === "control_request" && l.request?.subtype === "can_use_tool");
  expect(asks).toHaveLength(1);
  // The prompt came from Ovra's PreToolUse hook, not from the CLI's own rules.
  expect(asks[0]?.request).toMatchObject({ tool_name: "Write", decision_reason_type: "hook" });
});

test("interrupt a running command, then go on in the same session", async ({ page }) => {
  const { base, dir } = makeProject("interrupt", { "README.md": "# Wait\n" });
  const before = recorded();
  await open(page, dir);
  const session = await startSession(
    page,
    "Run the shell command `ping -c 120 127.0.0.1` with the Bash tool in the foreground (not in the " +
      "background), then tell me its last line.",
  );
  // Whether the CLI asks before a ping depends on its own rules: allow it if it does.
  const card = page.getByTestId("approvals").getByRole("group", { name: /Run ping -c 120/ });
  const running = page
    .getByTestId("transcript")
    .getByRole("listitem")
    .filter({ hasText: /Tool Bash\s*Run ping -c 120/ });
  await expect(card.or(running).first()).toBeVisible();
  if (await card.isVisible()) await answer(page, /Run ping -c 120/, "Allow");
  await expect(session.getByRole("status")).toHaveText("Working");
  await session.getByRole("button", { name: "Interrupt" }).click();
  await waitingForYou(session);

  await reply(session, "Never mind. Reply with just the word: ready");
  await expect(page.getByTestId("transcript")).toContainText(/ready/i);
  await waitingForYou(session);
  await stop(session);

  const lines = cliLines(recording(before, base, "interrupted"));
  const results = lines.filter((l) => l.type === "result");
  expect(results).toHaveLength(2);
  // The interrupt reached the command while it ran, not before the model answered.
  expect(lines.some((l) => l.type === "assistant" && JSON.stringify(l).includes("ping -c 120"))).toBe(true);
});

test("a subagent's messages carry their parent", async ({ page }) => {
  const { base, dir } = makeProject("subagent", { "README.md": "# Subagents\n\nFirst line after the title.\n" });
  const before = recorded();
  await open(page, dir);
  const session = await startSession(
    page,
    "Use the Task tool to start exactly one general-purpose subagent that reads README.md and returns " +
      "its last line. Do not read the file yourself. Then tell me that line.",
  );
  await waitingForYou(session);
  await expect(page.getByTestId("transcript")).toContainText("First line after the title.");
  await stop(session);

  const lines = cliLines(recording(before, base, "subagent"));
  expect(lines.some((l) => typeof l.parent_tool_use_id === "string")).toBe(true);
});

/** One turn of `claude` with the flags Ovra uses (launch.rs), resuming `id`. */
async function resumeTurn(dir: string, id: string, prompt: string): Promise<{ init: Line; result: Line }> {
  const args = [
    "-p",
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--permission-prompt-tool",
    "stdio",
    "--permission-mode",
    "default",
    "--setting-sources",
    "user",
    "--strict-mcp-config",
    "--mcp-config",
    '{"mcpServers":{}}',
    "--resume",
    id,
  ];
  const child = spawn(env("OVRA_LIVE_CLAUDE"), args, { cwd: dir, stdio: ["pipe", "pipe", "inherit"] });
  const send = (line: unknown) => child.stdin.write(`${JSON.stringify(line)}\n`);
  send({ type: "control_request", request_id: "init", request: { subtype: "initialize" } });
  send({ type: "user", message: { role: "user", content: prompt } });
  let init: Line | undefined;
  for await (const text of createInterface({ input: child.stdout })) {
    const line = JSON.parse(text) as Line;
    if (line.type === "system" && line.subtype === "init") init = line;
    if (line.type === "control_request") {
      send({
        type: "control_response",
        response: { subtype: "success", request_id: line.request_id, response: { behavior: "deny", message: "No." } },
      });
    }
    if (line.type === "result") {
      child.stdin.end();
      if (!init) throw new Error("no system/init before the result");
      return { init, result: line };
    }
  }
  throw new Error("claude exited before the turn ended");
}

test("a resumed session remembers the earlier one", async () => {
  test.skip(!editSession, "needs the edit session");
  const { dir, id } = editSession as { dir: string; id: string };
  const { init, result } = await resumeTurn(
    dir,
    id,
    "Which file did you fix earlier in this session? Answer with the file name only, no tools.",
  );
  expect(init.session_id).toBe(id);
  expect(String(result.result)).toContain("README.md");
});
