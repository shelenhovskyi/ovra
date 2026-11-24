import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AgentSetup } from "../../lib/protocol";
import { Workspace } from "../../shell/workspace.svelte";
import { FakeAgentCore } from "../fake-core";
import TestHost from "../TestHost.svelte";
import { advice } from "./texts";

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
  return { path: null, executable: "/usr/bin/claude", version: "2.1.331", state, ...extra };
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

describe("ClaudeSetup", () => {
  it("shows a ready Claude in one line and hides the path field", async () => {
    await show(new FakeAgentCore());
    expect(card()?.querySelector("h3")?.textContent).toBe("Claude 2.1.331 is ready");
    expect(card()?.querySelector("#claude-path")).toBeNull();
    button(card(), "Change path")?.click();
    flushSync();
    expect(card()?.querySelector("#claude-path")).not.toBeNull();
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
    expect(core.sent("projects.trust").map((c) => c.params)).toEqual([{ id: "p1", trusted: true }]);
    expect(workspace.project?.trusted).toBe(true);
    expect(trustCard()?.querySelector("h3")?.textContent).toBe("Project settings: trusted");
    button(trustCard(), "Stop trusting")?.click();
    await settle();
    expect(workspace.project?.trusted).toBe(false);
  });

  it("says that an empty project still trusts future settings", async () => {
    await show(new FakeAgentCore());
    button(trustCard(), "Review and trust…")?.click();
    await settle();
    expect(trustCard()?.textContent).toContain("no Claude settings of its own yet");
  });
});
