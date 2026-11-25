import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { browserPicks } from "../browser/picks.svelte";
import type { Pick } from "../lib/protocol";
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
  await new Promise((resolve) => setImmediate(resolve));
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

/** Picks how the next task runs, as a click on its option would. */
function choose(label: string) {
  const option = [...target.querySelectorAll<HTMLLabelElement>(".modes label")].find(
    (l) => l.textContent?.trim() === label,
  );
  option?.querySelector("input")?.click();
  flushSync();
}

const startButton = () =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Start");

const panel = () => target.querySelector<HTMLElement>("[data-testid=agents-panel]");
const transcript = () =>
  [...target.querySelectorAll("[data-testid=transcript] li")].map((li) => li.textContent?.replace(/\s+/g, " ").trim());

describe("AgentsPanel", () => {
  it("offers one Start button and two plainly worded ways to work, one agent here by default", async () => {
    await setup();
    const buttons = [...target.querySelectorAll("form.new button")].map((b) => b.textContent);
    expect(buttons).toEqual(["Start"]);
    expect(target.querySelector("label[for=agent-prompt]")?.textContent).toBe("What should Claude do?");
    const options = () =>
      [...target.querySelectorAll<HTMLInputElement>("input[name=agent-mode]")].map((i) => [
        i.closest("label")?.textContent?.trim(),
        i.checked,
      ]);
    const hint = () => target.querySelector("#agent-mode-hint")?.textContent;
    expect(options()).toEqual([
      ["One agent, here", true],
      ["Plan and split", false],
    ]);
    expect(hint()).toBe("Claude works directly in this folder. You approve risky steps as it goes.");
    choose("Plan and split");
    expect(options()).toEqual([
      ["One agent, here", false],
      ["Plan and split", true],
    ]);
    expect(hint()).toContain("up to 3 agents, each on its own copy of the project");
    expect(panel()?.querySelector("[data-testid=agents-empty]")?.textContent?.trim()).toBe(
      "Describe a task and press Start. Claude asks before risky steps; its answers appear here.",
    );
  });

  it("plans and splits a task when asked, from Mod+Enter too, and keeps the choice", async () => {
    const core = await setup();
    choose("Plan and split");
    type(target.querySelector("#agent-prompt"), "Write the notes");
    modEnter();
    await settle();
    expect(core.sent("orchestration.start")).toHaveLength(1);
    expect(core.sent("agents.start")).toEqual([]);
    expect(target.querySelector<HTMLInputElement>("input[value=split]")?.checked).toBe(true);
    expect(panel()?.querySelector("[data-testid=agents-empty]")).toBeNull();
  });

  it("starts a task whose coordinator splits the work among workers", async () => {
    const core = await setup();
    choose("Plan and split");
    type(target.querySelector("#agent-prompt"), "Write the notes");
    flushSync();
    startButton()?.click();
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
    choose("Plan and split");
    type(target.querySelector("#agent-prompt"), "Write the notes");
    flushSync();
    startButton()?.click();
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
    expect(billed?.textContent?.trim()).toBe("This agent: Usage is billed to an Anthropic API key, at API prices.");
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
    core.sandboxed("s2", { name: "bubblewrap, Landlock, seccomp", network: "proxy", gaps: [] });
    await settle();
    expect(line()?.textContent).toBe(
      "Runs in a sandbox (bubblewrap, Landlock, seccomp), network only through Ovra's proxy.",
    );
    expect(line()?.classList.contains("unsandboxed")).toBe(false);
    expect(badges()).toEqual(["Sandboxed"]);
    const gaps = () => panel()?.querySelector("[data-testid=agent-sandbox-gaps]")?.textContent;
    expect(gaps()).toBeUndefined();
    core.sandboxed("s2", {
      name: "fake sandbox",
      network: "proxy",
      gaps: ["DNS lookups", "loopback services"],
    });
    await settle();
    expect(gaps()).toBe("Not covered: DNS lookups, loopback services.");
    core.status("s2", { state: "stopped" });
    await settle();
    expect(gaps()).toBe("Did not cover: DNS lookups, loopback services.");
    core.sandboxed("s2", { name: "bubblewrap, Landlock, seccomp", network: "proxy", gaps: [] });
    await settle();
    expect(line()?.textContent).toBe(
      "Ran in a sandbox (bubblewrap, Landlock, seccomp), network only through Ovra's proxy.",
    );
  });

  it("forgets the sandbox report when a session starts again", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Earlier task", { state: "idle" });
    await setup(core);
    const line = () => panel()?.querySelector("[data-testid=agent-sandbox]")?.textContent;
    core.sandboxed("s1", null);
    await settle();
    expect(line()).toBe("No sandbox: the agent can read and change anything your user can.");
    // A restart clears the report in the core and sends only a status.
    core.status("s1", { state: "starting" });
    await settle();
    expect(line()).toBe("Sandbox: starting…");
  });

  it("keeps a sandbox report that came before the reply to agents.start", async () => {
    const core = new FakeAgentCore();
    await setup(core);
    const call = core.call.bind(core);
    core.call = (async (request: Parameters<typeof call>[0]) => {
      const reply = await call(request);
      if (request.method === "agents.start") core.sandboxed("s1", { name: "bwrap", network: "proxy", gaps: [] });
      return reply;
    }) as typeof core.call;
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    modEnter();
    await settle();
    expect(panel()?.querySelector("[data-testid=agent-sandbox]")?.textContent).toBe(
      "Runs in a sandbox (bwrap), network only through Ovra's proxy.",
    );
  });

  it("marks the cost as an API price estimate on a Claude plan", async () => {
    const core = new FakeAgentCore();
    const info = core.addSession("p1", "Earlier task", { state: "idle" });
    info.billing = { kind: "subscription" };
    info.usage = { input_tokens: 64044, output_tokens: 372, cost_micro_usd: 2400 };
    core.transcripts.set("s1", [{ kind: "turn_end", usage: info.usage }]);
    await setup(core);
    const estimate = "64,044 tokens in, 372 out, about $0.0024 at API prices (your plan's limits, not this amount)";
    expect(panel()?.querySelector("[data-testid=agent-usage]")?.textContent).toBe(`Total: ${estimate}`);
    expect(transcript()).toEqual([`Turn ended: ${estimate}`]);
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

  it("shows the elements picked in the session's browser and attaches them to the reply", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix the header", { state: "idle" });
    await setup(core);
    const pick = (tag: string, locator: string): Pick => ({
      pick_id: `pk-${tag}`,
      url: "http://localhost:5173/",
      selector: `header > ${tag}`,
      locator,
      tag,
      text: "",
      html: "",
      rect: { x: 0, y: 0, width: 10, height: 10 },
    });
    const picks = browserPicks(core);
    picks.add("s1", "pg1", pick("button", 'button "Save"'));
    picks.add("s1", "pg1", pick("h1", 'heading "Welcome"'));
    picks.note("s1", "pk-button", "make it blue");
    await settle();
    const chips = () => [...target.querySelectorAll("[data-testid=pick-chip] span")].map((c) => c.textContent);
    expect(chips()).toEqual(['button "Save"', 'heading "Welcome"']);
    target.querySelector<HTMLButtonElement>("button[aria-label='Remove heading \"Welcome\"']")?.click();
    await settle();
    expect(chips()).toEqual(['button "Save"']);
    type(target.querySelector("#agent-reply"), "this one");
    modEnter();
    await settle();
    // The picks go first: the session's next message carries them.
    expect(core.calls.filter((c) => c.method === "browser.attach" || c.method === "agents.send")).toEqual([
      {
        method: "browser.attach",
        params: { page: "pg1", picks: [{ ...pick("button", 'button "Save"'), note: "make it blue" }] },
      },
      { method: "agents.send", params: { session: "s1", text: "this one" } },
    ]);
    expect(chips()).toEqual([]);
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

  it("says next to the form when Claude fails right after starting, and points to the setup", async () => {
    const core = await setup();
    type(target.querySelector("#agent-prompt"), "Fix the typo");
    flushSync();
    startButton()?.click();
    await settle();
    const alert = () => panel()?.querySelector("[data-testid=agent-start-error]")?.textContent?.replace(/\s+/g, " ");
    core.status("s1", { state: "working" });
    await settle();
    core.status("s1", { state: "failed", message: "later" });
    await settle();
    // Failing after it got going is not a start problem.
    expect(alert()).toBeUndefined();
    type(target.querySelector("#agent-prompt"), "Again");
    modEnter();
    await settle();
    core.status("s2", { state: "failed", message: "claude was not found." });
    await settle();
    expect(alert()?.trim()).toBe(
      "Claude could not start: claude was not found. If Claude is not set up yet, the Claude section above says what to do; then press Start again.",
    );
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
    expect(panel()?.querySelector("[role=alert]")?.textContent?.trim()).toBe(
      "Could not start: another agent already works in this project's main checkout",
    );
  });
});
