import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { ProviderInfo } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
import { agentPrefs } from "./prefs.svelte";
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
  localStorage.clear();
});

async function settle() {
  for (let i = 0; i < 3; i++) {
    flushSync();
    await new Promise((resolve) => setImmediate(resolve));
  }
  flushSync();
}

/** Gemini as the ACP adapter reports it: no enforced policy, no IDE tools. */
const GEMINI: ProviderInfo = {
  id: "gemini",
  name: "Gemini",
  capabilities: {
    approvals: true,
    enforced_policy: false,
    ide_tools: false,
    read_only_role: false,
    resume: false,
    fork: false,
    interrupt: true,
    text_deltas: true,
    subagents: false,
    usage: false,
    cost: false,
    billing: false,
    accounts: false,
    images: false,
  },
};

async function open(core: FakeAgentCore) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
}

const picker = () => target.querySelector<HTMLSelectElement>("[data-testid=agent-picker] select");
const modeButton = () => target.querySelector<HTMLButtonElement>("[data-testid=agent-mode]");

async function modes(): Promise<string[]> {
  modeButton()?.click();
  await settle();
  const names = [...target.querySelectorAll("[role=menuitemradio] .name")].map((e) => e.textContent ?? "");
  modeButton()?.click();
  await settle();
  return names;
}

it("offers no agent picker while Claude is the only ready agent", async () => {
  const core = new FakeAgentCore();
  core.providers = [...core.providers, GEMINI];
  core.setups.gemini = { ...core.setup, state: { state: "not_found" }, executable: null, version: null };
  await open(core);
  expect(picker()).toBeNull();
  // Attempts wait until the person turns them on in Settings.
  expect(target.querySelector("[data-testid=attempts-picker]")).toBeNull();
  expect(await modes()).toEqual(["One agent, here", "One agent, own copy", "Plan and split", "Research"]);
});

it("starts Gemini only on its own copy, as one agent without a coordinator", async () => {
  const core = new FakeAgentCore();
  core.providers = [...core.providers, GEMINI];
  core.setups.gemini = { ...core.setup, executable: "/usr/bin/gemini", version: "0.71.1", billing: null };
  await open(core);
  const select = picker();
  expect([...(select?.options ?? [])].map((o) => o.textContent)).toEqual(["Claude", "Gemini"]);
  if (!select) throw new Error("no picker");
  select.value = "gemini";
  select.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  expect(await modes()).toEqual(["One agent, own copy"]);
  expect(target.querySelector("#agent-mode-hint")?.textContent).toContain("Gemini works only on its own copy");
  expect(target.querySelector("[data-testid=account-picker]")).toBeNull();
  expect(target.querySelector("[data-testid=agents-footer]")?.textContent).toBe("Gemini · its own sign-in");

  const field = target.querySelector<HTMLTextAreaElement>("#agent-prompt");
  if (!field) throw new Error("no field");
  field.value = "Fix the typo";
  field.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  target.querySelector<HTMLFormElement>("form")?.requestSubmit();
  await settle();
  const start = core.calls.find((c) => c.method === "orchestration.start");
  expect(start?.params).toMatchObject({ project: "p1", prompt: "Fix the typo", agent: "gemini", solo: true });
  expect(core.calls.some((c) => c.method === "agents.start")).toBe(false);
  // The choice stays for the project.
  expect(localStorage.getItem("ovra.task-agent.p1")).toBe("gemini");
});

async function submit(prompt: string) {
  const field = target.querySelector<HTMLTextAreaElement>("#agent-prompt");
  if (!field) throw new Error("no field");
  field.value = prompt;
  field.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  target.querySelector<HTMLFormElement>("form")?.requestSubmit();
  await settle();
}

it("starts Claude when the remembered agent cannot start now, and keeps the pick", async () => {
  localStorage.setItem("ovra.task-agent.p1", "gemini");
  const core = new FakeAgentCore();
  core.providers = [...core.providers, GEMINI];
  core.setups.gemini = { ...core.setup, state: { state: "not_found" }, executable: null, version: null };
  await open(core);
  expect(picker()).toBeNull();
  expect(await modes()).toEqual(["One agent, here", "One agent, own copy", "Plan and split", "Research"]);
  expect(target.querySelector("[data-testid=agents-footer]")?.textContent).not.toContain("Gemini");
  await submit("Fix the typo");
  expect(core.sent("agents.start").map((c) => c.params.agent)).toEqual(["claude"]);
  expect(core.sent("orchestration.start")).toEqual([]);
  // Gemini comes back once it can start again.
  expect(localStorage.getItem("ovra.task-agent.p1")).toBe("gemini");
});

it("starts the picked agent in this folder when it can run there", async () => {
  localStorage.setItem("ovra.task-agent.p1", "gemini");
  const core = new FakeAgentCore();
  core.providers = [...core.providers, { ...GEMINI, capabilities: { ...GEMINI.capabilities, enforced_policy: true } }];
  core.setups.gemini = { ...core.setup, executable: "/usr/bin/gemini", version: "0.71.1", billing: null };
  await open(core);
  expect(picker()?.value).toBe("gemini");
  expect(await modes()).toEqual(["One agent, here", "One agent, own copy"]);
  await submit("Fix the typo");
  expect(core.sent("agents.start").map((c) => [c.params.agent, c.params.mode])).toEqual([["gemini", "direct"]]);
});

it("starts several attempts at one task, each agent in its own copy, once Settings offers them", async () => {
  agentPrefs.attempts = true;
  const core = new FakeAgentCore();
  core.providers = [...core.providers, GEMINI];
  core.setups.gemini = { ...core.setup, executable: "/usr/bin/gemini", version: "0.71.1", billing: null };
  try {
    await open(core);
    // Only "One agent, own copy" has attempts.
    expect(target.querySelector("[data-testid=attempts-picker]")).toBeNull();
    modeButton()?.click();
    await settle();
    target.querySelector<HTMLButtonElement>("[data-mode=own]")?.click();
    await settle();
    const tries = target.querySelector<HTMLSelectElement>("[data-testid=attempts-picker] select");
    if (!tries) throw new Error("no attempts picker");
    tries.value = "3";
    tries.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();
    expect(target.querySelector("#agent-mode-hint")?.textContent).toContain("3 attempts (Claude, Gemini, Claude)");

    const field = target.querySelector<HTMLTextAreaElement>("#agent-prompt");
    if (!field) throw new Error("no field");
    field.value = "Fix the typo";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    target.querySelector<HTMLFormElement>("form")?.requestSubmit();
    await settle();
    const starts = core.calls.filter((c) => c.method === "orchestration.start").map((c) => c.params);
    expect(starts).toEqual([
      { project: "p1", prompt: "Fix the typo", agent: "claude", solo: true },
      { project: "p1", prompt: "Fix the typo", agent: "gemini", solo: true, attempt_of: "t1" },
      { project: "p1", prompt: "Fix the typo", agent: "claude", solo: true, attempt_of: "t1" },
    ]);
    // The first attempt opens, named as one of several.
    expect(target.textContent).toContain("Attempt 1 (Claude) · Fix the typo");
  } finally {
    agentPrefs.attempts = false;
  }
});
