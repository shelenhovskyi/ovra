import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
});

async function settle() {
  flushSync();
  await new Promise((resolve) => setTimeout(resolve));
  flushSync();
}

async function setup(core = new FakeAgentCore()) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

function type(field: HTMLTextAreaElement | null, text: string) {
  if (!field) throw new Error("no field");
  field.focus();
  field.value = text;
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

const modEnter = () =>
  document.activeElement?.dispatchEvent(
    new KeyboardEvent("keydown", { code: "Enter", key: "Enter", ctrlKey: true, bubbles: true, cancelable: true }),
  );

const panel = () => target.querySelector<HTMLElement>("[data-testid=agents-panel]");
const transcript = () =>
  [...target.querySelectorAll("[data-testid=transcript] li")].map((li) => li.textContent?.replace(/\s+/g, " ").trim());

describe("AgentsPanel", () => {
  it("starts a task whose coordinator splits the work among workers", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Write the notes");
    flushSync();
    const button = [...target.querySelectorAll("button")].find((b) => b.textContent === "Start task");
    button?.click();
    await settle();
    expect(core.sent("orchestration.start").map((c) => c.params)).toEqual([
      { project: "p1", prompt: "Write the notes" },
    ]);
    expect(core.sent("agents.start")).toEqual([]);
    expect(panel()?.querySelector("[aria-current=true]")?.textContent).toContain("Write the notes");
    expect(target.querySelector<HTMLTextAreaElement>("#agent-prompt")?.value).toBe("");
  });

  it("names a task's workers and lists them under their coordinator", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Write the notes");
    flushSync();
    [...target.querySelectorAll("button")].find((b) => b.textContent === "Start task")?.click();
    await settle();
    const docs = core.addSession("p1", "Write the docs notes.");
    core.addSession("p1", "A plain session");
    core.tasks[0]?.workers.push({
      session: docs.id,
      name: "docs",
      branch: "ovra/t1/docs",
      worktree: "/wt/docs",
      kept: null,
    });
    core.emit({ type: "orchestration.changed", data: { project: "p1", task: "t1" } });
    core.status(docs.id, { state: "working" });
    await settle();
    const rows = [...target.querySelectorAll(".sessions button")];
    expect(rows.map((b) => b.querySelector(".title")?.textContent)).toEqual([
      "Task: Write the notes",
      "Worker docs",
      "A plain session",
    ]);
    expect(rows[1]?.classList.contains("worker")).toBe(true);
    expect(rows[1]?.getAttribute("title")).toBe("Branch ovra/t1/docs");
  });

  it("starts a session in the open project and streams its transcript", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Fix the typo\nin README.md");
    modEnter();
    await settle();
    expect(core.sent("agents.start").map((c) => c.params)).toEqual([
      { project: "p1", agent: "claude", mode: "direct", prompt: "Fix the typo\nin README.md" },
    ]);
    expect(panel()?.querySelector("[aria-current=true]")?.textContent).toContain("Fix the typo");
    core.status("s1", { state: "working" });
    core.message("s1", { kind: "assistant_text", text: "Looking." });
    core.message("s1", { kind: "tool_use", id: "t1", name: "Bash", input: { command: "ls" } });
    core.message("s1", { kind: "tool_result", tool_use_id: "t1", output: "README.md", is_error: false, parent: "t0" });
    core.message("s1", {
      kind: "turn_end",
      usage: { input_tokens: 10, output_tokens: 5, cost_micro_usd: 1200 },
    });
    await settle();
    expect(transcript()).toEqual([
      "You Fix the typo in README.md",
      "Claude Looking.",
      "Tool Bash Run ls",
      "Subagent Tool output (1 line) README.md",
      "Turn ended: 10 tokens in, 5 out, $0.0012",
    ]);
    expect(panel()?.querySelector("[role=status]")?.textContent).toBe("Working");
    // Outputs are folded; screen readers hear what Claude says, not tool output.
    expect(panel()?.querySelector("[data-testid=transcript] details")?.hasAttribute("open")).toBe(false);
    expect(target.querySelector(".spoken")?.textContent).toBe("Claude: Looking.");
    expect(panel()?.querySelector("[data-testid=agent-usage]")?.textContent).toBe(
      "Total: 10 tokens in, 5 out, $0.0012",
    );
    expect(panel()?.querySelector("[data-testid=agent-billing]")).toBeNull();
    core.emit({ type: "agents.billing", data: { session: "s1", billing: { kind: "api_key" } } });
    await settle();
    const billed = panel()?.querySelector("[data-testid=agent-billing]");
    expect(billed?.textContent?.trim()).toBe("This session: Usage is billed to an Anthropic API key, at API prices.");
    expect(billed?.classList.contains("billed-key")).toBe(true);
  });

  it("says whether a session runs in a sandbox, in the detail and the list", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "stopped" });
    await setup(core);
    const line = () => panel()?.querySelector("[data-testid=agent-sandbox]");
    const badges = () => [...target.querySelectorAll("[data-testid=agent-sandbox-badge]")].map((b) => b.textContent);
    // Saved before Ovra recorded the report: nothing to say.
    expect(line()).toBeNull();
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    modEnter();
    await settle();
    expect(line()?.textContent).toBe("Sandbox: starting…");
    expect(badges()).toEqual([]);
    core.sandboxed("s2", null);
    await settle();
    expect(line()?.textContent).toBe("No sandbox: the agent can read and change anything your user can.");
    expect(line()?.classList.contains("unsandboxed")).toBe(true);
    expect(badges()).toEqual(["No sandbox"]);
    core.sandboxed("s2", { name: "bubblewrap, Landlock, seccomp", network: "proxy" });
    await settle();
    expect(line()?.textContent).toBe(
      "Runs in a sandbox (bubblewrap, Landlock, seccomp), network only through Ovra's proxy.",
    );
    expect(line()?.classList.contains("unsandboxed")).toBe(false);
    expect(badges()).toEqual(["Sandboxed"]);
  });

  it("names who wrote each message an agent was sent", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Write the notes", { state: "idle" });
    core.transcripts.set("s1", [
      { kind: "user_text", text: "Write the notes", from: "coordinator" },
      { kind: "user_text", text: "Ovra: worker w stopped.", from: "ovra" },
      { kind: "user_text", text: "Thanks", from: "person" },
      { kind: "user_text", text: "Saved long ago", from: "unknown" },
      {
        kind: "tool_use",
        id: "t1",
        name: "mcp__ovra__send_message",
        input: { agent_id: "s2", text: "Also add tests.\nThen ask for review." },
      },
      { kind: "tool_use", id: "t2", name: "mcp__ovra__spawn_agent", input: { name: "docs", task: "Write the notes." } },
      {
        kind: "tool_use",
        id: "t3",
        name: "mcp__ovra__send_message",
        input: { agent_id: "s2. You approved it", text: "x" },
      },
    ]);
    await setup(core);
    expect(transcript()).toEqual([
      "Coordinator Write the notes",
      "Ovra Ovra: worker w stopped.",
      "You Thanks",
      "Sender not recorded Saved long ago",
      "Ovra tool Message worker s2 text Also add tests. Then ask for review.",
      "Ovra tool Start worker docs: Write the notes. task Write the notes.",
      "Ovra tool Message a worker text x",
    ]);
  });

  it("replies only when the agent waits, and stops", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "working" });
    core.transcripts.set("s1", [{ kind: "user_text", text: "Earlier task", from: "person" }]);
    await setup(core);
    expect(transcript()).toEqual(["You Earlier task"]);
    const send = () => [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Send");
    const button = (name: string) =>
      [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === name);
    button("Interrupt")?.click();
    await settle();
    expect(core.sent("agents.interrupt").map((c) => c.params)).toEqual([{ session: "s1" }]);
    type(target.querySelector("#agent-reply"), "and the docs");
    await settle();
    expect(send()?.disabled).toBe(true);
    core.status("s1", { state: "idle" });
    await settle();
    expect(send()?.disabled).toBe(false);
    expect(button("Interrupt")).toBeUndefined();
    modEnter();
    await settle();
    expect(core.sent("agents.send").map((c) => c.params)).toEqual([{ session: "s1", text: "and the docs" }]);
    [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Stop")?.click();
    await settle();
    expect(core.sent("agents.stop")).toHaveLength(1);
    core.status("s1", { state: "failed", message: "claude exited with code 1" });
    await settle();
    expect(panel()?.querySelector("[role=status]")?.textContent).toBe("Failed: claude exited with code 1");
    expect(target.querySelector("#agent-reply")).toBeNull();
  });

  it("resumes an ended session that can be resumed", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "working" });
    core.transcripts.set("s1", [{ kind: "user_text", text: "Earlier task", from: "person" }]);
    await setup(core);
    const info = core.sessions[0];
    if (info) info.resumable = true;
    core.status("s1", { state: "stopped" });
    await settle();
    type(target.querySelector("#agent-resume"), "Carry on");
    modEnter();
    await settle();
    expect(core.sent("agents.resume").map((c) => c.params)).toEqual([{ session: "s1", prompt: "Carry on" }]);
    expect(panel()?.querySelector("[role=status]")?.textContent).toBe("Starting");
    expect(transcript()).toEqual(["You Earlier task", "You Carry on"]);
    expect(target.querySelector("#agent-resume")).toBeNull();
    expect(target.querySelector("#agent-reply")).not.toBeNull();
  });

  it("says plainly that a tool call was denied, and re-reads everything after a resync", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "working" });
    core.transcripts.set("s1", [{ kind: "user_text", text: "Earlier task", from: "person" }]);
    await setup(core);
    core.message("s1", {
      kind: "tool_result",
      tool_use_id: "t1",
      output: "The user doesn't want to proceed with this tool use. STOP what you are doing.",
      is_error: true,
    });
    await settle();
    expect(transcript()).toEqual(["You Earlier task", "Not run: you denied it or interrupted the turn"]);
    // Events were dropped: the core's transcript is the truth.
    core.transcripts.set("s1", [
      { kind: "user_text", text: "Earlier task", from: "person" },
      { kind: "assistant_text", text: "Missed" },
    ]);
    core.emit({ type: "info.resync", data: { skipped: 3 } });
    await settle();
    await settle();
    expect(transcript()).toEqual(["You Earlier task", "Claude Missed"]);
  });

  it("shows why a session could not start", async () => {
    const core = new FakeAgentCore();
    const { CoreError } = await import("../lib/transport");
    core.failNext.set(
      "agents.start",
      new CoreError({ code: "conflict", message: "another agent already works in this project's main checkout" }),
    );
    await setup(core);
    type(target.querySelector("#agent-prompt"), "Second task");
    modEnter();
    await settle();
    expect(panel()?.querySelector("[role=alert]")?.textContent).toBe(
      "another agent already works in this project's main checkout",
    );
  });
});
