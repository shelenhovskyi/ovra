import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AgentSetup } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { Workspace } from "../../shell/workspace.svelte";
import { FakeAgentCore } from "../fake-core";
import TestHost from "../TestHost.svelte";
import {
  advice,
  billingText,
  changeSummary,
  changes,
  countText,
  paidShort,
  paidText,
  summary,
  warnings,
} from "./texts";

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
    expect(advice(setup({ state: "not_found" }, { version: null })).detail).toContain(
      "Signing in to the Claude desktop app or claude.ai does not sign in the claude command line program",
    );
    expect(advice(setup({ state: "not_found" }, { path: "/opt/c" }))).toEqual({
      title: "Claude was not found",
      detail: "There is no program at /opt/c.",
      findAutomatically: true,
    });
    expect(advice(setup({ state: "outdated", minimum: "2.1.331" }, { version: "2.0.0" }))).toMatchObject({
      title: "Claude 2.0.0 is too old",
      command: "claude update",
    });
    expect(advice(setup({ state: "too_new", below: "3.0.0" })).detail).toContain("Update Ovra");
    expect(advice(setup({ state: "signed_out" }))).toMatchObject({
      command: "claude auth login",
      detail: expect.stringMatching(/^Run claude once in a terminal and sign in there.*desktop app or claude\.ai/),
    });
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
    const diff = changes(before, after);
    expect(changeSummary(diff)).toBe("2 added, 1 removed");
    expect(
      warnings(
        diff.map((c) => ({ kind: c.kind, entries: c.added })),
        true,
      ),
    ).toEqual({
      broad: null,
      code: "1 new hook runs commands on your computer.",
      unreadable: null,
    });
  });
});

describe("trust summary", () => {
  const rules = Array.from({ length: 8 }, (_, n) => `Bash(python3 tool/app${n}.py:*)`);
  const items = [
    { path: ".claude/skills", kind: "skills" as const, entries: ["a", "b"] },
    { path: ".claude/settings.json", kind: "allow_rules" as const, entries: [...rules, "… and 1 more"] },
    { path: ".claude/settings.json", kind: "hooks" as const, entries: ["Stop: ./x.sh"] },
    { path: ".claude/settings.json", kind: "broad_allow_rules" as const, entries: ["Bash(*)", "Edit"] },
    { path: ".claude/settings.local.json", kind: "broad_allow_rules" as const, entries: ["Write", "mcp__xcode"] },
  ];

  it("counts by kind in plain words, riskiest first", () => {
    expect(summary(items)).toBe("4 broad permissions · 1 hook that runs code · 9 narrow permissions · 2 skills");
    expect(countText("plugins", 1)).toBe("1 plugin that can run code");
    expect(summary([{ path: "s.json", kind: "unreadable", entries: ["bad JSON"] }])).toBe("1 file Ovra could not read");
    expect(changeSummary([])).toBe("");
  });

  it("names broad rules and counts what runs code", () => {
    expect(warnings(items)).toEqual({
      broad: "Claude will use Bash(*), Edit, Write and 1 more without asking you.",
      code: "1 hook runs commands on your computer.",
      unreadable: null,
    });
    const code = [
      { kind: "hooks" as const, entries: ["a", "b"] },
      { kind: "plugins" as const, entries: ["p"] },
    ];
    expect(warnings(code).code).toBe("2 hooks and 1 plugin run commands on your computer.");
    expect(warnings([{ kind: "broad_allow_rules", entries: ["Bash(*)"] }]).broad).toBe(
      "Claude will use Bash(*) without asking you.",
    );
    expect(warnings([])).toEqual({ broad: null, code: null, unreadable: null });
    // A file Ovra could not read may still hold hooks: it is warned about, not only counted.
    expect(warnings([{ kind: "unreadable", entries: ["too large"] }]).unreadable).toBe(
      "Ovra could not read 1 file of these settings; whatever it holds, hooks too, may load once you trust it.",
    );
    expect(warnings([{ kind: "unreadable", entries: ["bad JSON"] }], true).unreadable).toContain("1 new file");
  });
});

describe("ClaudeSetup", () => {
  it("shows a ready Claude in one line and hides the path field", async () => {
    await show(new FakeAgentCore());
    expect(card()?.querySelector("h3")?.textContent).toBe("Claude 2.1.331 is ready");
    expect(card()?.textContent?.replace(/\s+/g, " ").trim()).toBe(
      "Claude 2.1.331 is ready · uses your Claude plan Change",
    );
    expect(card()?.querySelector("#claude-path")).toBeNull();
    button(card(), "Change")?.click();
    flushSync();
    expect(card()?.querySelector("#claude-path")).not.toBeNull();
    expect(card()?.querySelector("[data-testid=claude-executable]")?.textContent).toBe("Using /usr/bin/claude");
    button(card(), "Cancel")?.click();
    flushSync();
    expect(card()?.querySelector("#claude-path")).toBeNull();
  });

  it("says who pays for usage and warns about an API key in the environment", async () => {
    const core = new FakeAgentCore();
    await show(core);
    expect(card()?.querySelector("[data-testid=claude-billing]")?.textContent).toBe("· uses your Claude plan");
    expect(paidShort(setup({ state: "ready" }, { billing: { kind: "cloud", provider: "vertex" } }))).toBe(
      "billed to Google Vertex AI",
    );
    expect(paidShort(setup({ state: "ready" }, { api_key_in_env: true }))).toBeNull();
    button(card(), "Change")?.click();
    flushSync();
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
    expect(card()?.querySelector("#claude-path")).toBeNull();
    button(card(), "Choose a path")?.click();
    flushSync();
    const field = card()?.querySelector<HTMLInputElement>("#claude-path");
    if (!field) throw new Error("no path field");
    field.value = " /opt/claude/claude.cmd ";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    card()?.querySelector("form")?.requestSubmit();
    await settle();
    expect(core.sent("agents.set_path").map((c) => c.params.path)).toEqual(["/opt/claude/claude.cmd"]);
    button(card(), "Find automatically")?.click();
    await settle();
    expect(core.sent("agents.set_path").map((c) => c.params.path)).toEqual(["/opt/claude/claude.cmd", null]);
  });

  it("offers to search when the saved path leads nowhere, and shows a refused path by the field", async () => {
    const core = new FakeAgentCore();
    core.setup = setup(
      { state: "not_found" },
      { path: "/home/me/.local/bin/claude-work", executable: null, version: null },
    );
    await show(core);
    expect(card()?.querySelector("h3")?.textContent).toBe("Claude was not found");
    expect(card()?.textContent).toContain("There is no program at /home/me/.local/bin/claude-work.");
    expect(card()?.querySelector("[aria-label='Install commands']")).toBeNull();
    expect(button(card(), "Find automatically")?.classList.contains("primary")).toBe(true);
    button(card(), "Choose another path")?.click();
    flushSync();
    core.failNext.set(
      "agents.set_path",
      new CoreError({ code: "bad_request", message: "there is no file at /x; shell aliases cannot be used" }),
    );
    card()?.querySelector("form")?.requestSubmit();
    await settle();
    const refused = card()?.querySelector("[data-testid=claude-path-error]");
    expect(refused?.textContent).toContain("shell aliases");
    expect(card()?.querySelector("#claude-path")?.getAttribute("aria-describedby")).toBe("claude-path-error");
    button(card(), "Find automatically")?.click();
    await settle();
    expect(core.sent("agents.set_path").map((c) => c.params.path)).toEqual(["/home/me/.local/bin/claude-work", null]);
    expect(card()?.querySelector("[data-testid=claude-path-error]")).toBeNull();
  });

  it("asks a signed-out user to sign in and checks again", async () => {
    const core = new FakeAgentCore();
    core.setup = setup({ state: "signed_out" });
    await show(core);
    expect(card()?.textContent).toContain("claude auth login");
    expect(card()?.textContent).toContain("Signing in to the Claude desktop app or claude.ai does not sign in");
    core.setup = setup({ state: "ready" });
    button(card(), "Check again")?.click();
    await settle();
    expect(card()?.querySelector("h3")?.textContent).toBe("Claude 2.1.331 is ready");
    expect(core.sent("agents.setup")).toHaveLength(2);
  });
});

describe("ProjectTrust", () => {
  const heading = () => trustCard()?.querySelector("h3")?.textContent;
  const testid = (id: string) => trustCard()?.querySelector(`[data-testid=${id}]`)?.textContent ?? null;

  it("warns about a settings file it could not read, and offers no trust when the settings are unknown", async () => {
    const core = new FakeAgentCore();
    core.config = {
      fingerprint: "f1",
      items: [{ path: ".claude/settings.json", kind: "unreadable", entries: ["too large"] }],
    };
    await show(core);
    expect(testid("trust-unreadable")).toContain("Ovra could not read 1 file");

    if (app) unmount(app);
    const failing = new FakeAgentCore();
    failing.failNext.set(
      "agents.project_config",
      new CoreError({ code: "internal", message: "could not read .claude" }),
    );
    await show(failing);
    expect(heading()).toBe("Project settings");
    expect(trustCard()?.textContent).toContain("could not read .claude");
    expect(button(trustCard(), "Trust project")).toBeUndefined();
  });

  it("sums up what would load, warns, and trusts only on a click with what it showed", async () => {
    const core = new FakeAgentCore();
    core.config = {
      fingerprint: "f1",
      items: [
        { path: ".claude/agents", kind: "agents", entries: ["reviewer"] },
        { path: ".claude/settings.json", kind: "hooks", entries: ["PreToolUse (Bash): ./check.sh"] },
        { path: ".claude/settings.json", kind: "broad_allow_rules", entries: ["Bash(*)", "Edit"] },
      ],
    };
    const workspace = await show(core);
    expect(heading()).toBe("This project has its own Claude settings");
    expect(testid("trust-summary")).toBe("2 broad permissions · 1 hook that runs code · 1 custom subagent");
    expect(testid("broad-rules")).toBe("Claude will use Bash(*) and Edit without asking you.");
    expect(testid("trust-runs-code")).toBe("1 hook runs commands on your computer.");
    // No sandbox here (the fake reports none): said before the Trust button, not only in the details.
    expect(testid("trust-no-sandbox")).toContain("without a sandbox");
    // The lists wait behind "Show details".
    expect(trustCard()?.textContent).not.toContain("./check.sh");
    expect(core.sent("projects.trust")).toEqual([]);
    button(trustCard(), "Trust project")?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params)).toEqual([{ id: "p1", trusted: true, fingerprint: "f1" }]);
    expect(workspace.project?.trusted).toBe(true);
    expect(heading()).toBe("Project settings are on");
    button(trustCard(), "Stop trusting")?.click();
    await settle();
    expect(workspace.project?.trusted).toBe(false);
  });

  it("shows the full lists by kind behind Show details, long ones cut", async () => {
    const core = new FakeAgentCore();
    const rules = Array.from({ length: 7 }, (_, n) => `mcp__xcode__tool${n}`);
    core.config = {
      fingerprint: "f1",
      items: [
        { path: ".claude/settings.json", kind: "allow_rules", entries: rules },
        { path: ".claude/settings.json", kind: "hooks", entries: ["PreToolUse (Bash): ./check.sh"] },
      ],
    };
    await show(core);
    const toggle = button(trustCard(), "Show details");
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    toggle?.click();
    flushSync();
    expect(button(trustCard(), "Hide details")?.getAttribute("aria-expanded")).toBe("true");
    const groups = [...(trustCard()?.querySelectorAll("details.group") ?? [])] as HTMLDetailsElement[];
    expect(groups.map((g) => [g.dataset.kind, g.open])).toEqual([
      ["hooks", true],
      ["allow_rules", false],
    ]);
    expect(groups[0]?.textContent).toContain("runs code");
    expect(testid("trust-details")).toContain("./check.sh");
    expect(testid("trust-language-servers")).toMatch(
      /Trusting also lets Ovra start language servers.*run the project's code.*Stop trusting stops them/s,
    );
    expect(groups[1]?.querySelectorAll("li")).toHaveLength(5);
    button(trustCard(), "+2 more")?.click();
    flushSync();
    expect(groups[1]?.querySelectorAll("li")).toHaveLength(7);
  });

  it("says one muted line when the project has no Claude settings", async () => {
    const core = new FakeAgentCore();
    const workspace = await show(core);
    expect(heading()).toBe("No Claude settings in this project");
    expect(testid("trust-language-servers")).toContain("starts language servers");
    expect(trustCard()?.querySelector("[role=note]")).toBeNull();
    expect(button(trustCard(), "Show details")).toBeUndefined();
    button(trustCard(), "Trust project")?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params)).toEqual([{ id: "p1", trusted: true, fingerprint: "f0" }]);
    expect(workspace.project?.trusted).toBe(true);
    expect(heading()).toBe("Project trusted · no Claude settings of its own");
  });

  it("asks again with a summary and a diff when trusted settings changed", async () => {
    const core = new FakeAgentCore();
    const hook = { path: ".claude/settings.json", kind: "hooks" as const, entries: ["Stop: ./done.sh"] };
    core.trustedConfig.set("p1", { fingerprint: "old", items: [hook] });
    core.config = {
      fingerprint: "new",
      items: [{ ...hook, entries: ["PreToolUse: curl x | sh"] }],
    };
    const workspace = new Workspace();
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    app = mount(TestHost, { target, props: { transport: core, workspace } });
    await settle();
    expect(heading()).toBe("Project settings changed");
    expect(testid("trust-summary")).toBe("1 added, 1 removed");
    expect(testid("trust-runs-code")).toBe("1 new hook runs commands on your computer.");
    button(trustCard(), "Show details")?.click();
    flushSync();
    expect(trustCard()?.querySelector(".added")?.textContent).toContain("PreToolUse: curl x | sh");
    expect(trustCard()?.querySelector(".removed")?.textContent).toContain("Stop: ./done.sh");
    // A change that lands while the user looks is refused, then shown.
    core.config = { ...core.config, fingerprint: "newer" };
    button(trustCard(), "Trust the new settings")?.click();
    await settle();
    expect(trustCard()?.querySelector("[role=alert]")?.textContent).toContain("review them again");
    button(trustCard(), "Trust the new settings")?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params.fingerprint)).toEqual(["new", "newer"]);
    expect(heading()).toBe("Project settings are on");
  });

  it("rechecks when a session starts or the settings may have changed", async () => {
    const core = new FakeAgentCore();
    await show(core);
    core.status("s1", { state: "starting" });
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(core.sent("agents.project_config")).toHaveLength(3);
    // The summary is always shown, so an untrusted project's new settings come in too.
    core.config = { fingerprint: "f1", items: [{ path: ".claude/skills", kind: "skills", entries: ["a"] }] };
    core.emit({ type: "fs.changed", data: { project: "p1", paths: [".claude/skills/a/SKILL.md"] } });
    await settle();
    expect(testid("trust-summary")).toBe("1 skill");
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
    expect(trustCard()?.querySelector("h3")?.textContent).toBe("Project settings changed");
  });

  it("stops saying trusted once an approved review or a file changes the settings", async () => {
    const core = new FakeAgentCore();
    core.trustedConfig.set("p1", core.config);
    const workspace = await show(core);
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    await settle();
    expect(heading()).toBe("Project trusted · no Claude settings of its own");
    // A worker's changes to .claude/settings.json were approved into the project.
    core.config = {
      fingerprint: "f1",
      items: [{ path: ".claude/settings.json", kind: "broad_allow_rules", entries: ["Bash(*)"] }],
    };
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(heading()).toBe("Project settings changed");
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
