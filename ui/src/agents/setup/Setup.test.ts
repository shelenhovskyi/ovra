import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listCommands } from "../../keys/commands";
import type { AgentSetup } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { languageServers } from "../../lsp/servers.svelte";
import { Workspace } from "../../shell/workspace.svelte";
import { FakeAgentCore } from "../fake-core";
import TestHost from "../TestHost.svelte";
import { advice, billingText, paidShort, paidText } from "./claude-texts";
import { changeSummary, changes, countText, riskyEnv, summary, TRUST_ARM_MS, warnings } from "./texts";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  vi.useFakeTimers();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
  vi.useRealTimers();
});

async function settle(ms = 0) {
  flushSync();
  await vi.advanceTimersByTimeAsync(ms);
  flushSync();
}

/** Waits until the trust buttons count a press. */
const armed = () => settle(TRUST_ARM_MS);

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
    expect(advice(setup({ state: "ready" }), "Claude").title).toBe("Claude 2.1.331 is ready");
    expect(advice(setup({ state: "not_found" }, { version: null }), "Claude").install).toBe(true);
    expect(advice(setup({ state: "not_found" }, { version: null }), "Claude").detail).toContain(
      "Signing in to the Claude desktop app or claude.ai does not sign in the claude command line program",
    );
    expect(advice(setup({ state: "not_found" }, { path: "/opt/c" }), "Claude")).toEqual({
      title: "Claude was not found",
      detail: "There is no program at /opt/c.",
      findAutomatically: true,
    });
    expect(advice(setup({ state: "outdated", minimum: "2.1.331" }, { version: "2.0.0" }), "Claude")).toMatchObject({
      title: "Claude 2.0.0 is too old",
      command: "claude update",
    });
    expect(advice(setup({ state: "too_new", below: "3.0.0" }), "Claude").detail).toContain("Update Ovra");
    expect(advice(setup({ state: "signed_out" }), "Claude beta")).toMatchObject({
      title: "Sign in to Claude beta",
      command: "claude auth login",
      detail: expect.stringMatching(/^Run claude once in a terminal and sign in there.*desktop app or claude\.ai/),
    });
    expect(advice(setup({ state: "broken", message: "boom" }), "Claude").detail).toBe("boom");
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
        diff.map((c) => ({ path: c.path, kind: c.kind, entries: c.added })),
        "changed",
      ),
    ).toEqual({
      broad: null,
      code: "Once you trust the changes, 1 new hook will run commands on your computer.",
      env: null,
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

  it("counts broad rules and what runs code in plain words, as what trusting would do", () => {
    expect(warnings(items, "untrusted")).toEqual({
      broad:
        "Once you trust this project, Claude will use 4 tools and commands without asking you, including any shell command.",
      code: "Once you trust this project, 1 hook will run commands on your computer.",
      env: null,
      unreadable: null,
    });
    // Once trusted, it is what happens now.
    expect(warnings(items, "trusted")).toMatchObject({
      broad: "Claude uses 4 tools and commands without asking you, including any shell command.",
      code: "1 hook runs commands on your computer.",
    });
    // Raw rules stay in the lists; the warning counts them, the core's "… and N more" too.
    const narrow = [{ path: "s.json", kind: "broad_allow_rules" as const, entries: ["Bash(make:*)", "… and 2 more"] }];
    expect(warnings(narrow, "changed").broad).toBe(
      "Once you trust the changes, Claude will use 3 new tools and commands without asking you.",
    );
    const code = [
      { path: "s.json", kind: "hooks" as const, entries: ["a", "b"] },
      { path: "s.json", kind: "plugins" as const, entries: ["p"] },
    ];
    expect(warnings(code, "trusted").code).toBe("2 hooks and 1 plugin run commands on your computer.");
    expect(warnings([{ path: "s.json", kind: "hooks", entries: ["a", "b", "c"] }], "trusted").code).toBe(
      "3 hooks run commands on your computer.",
    );
    expect(warnings([], "untrusted")).toEqual({ broad: null, code: null, env: null, unreadable: null });
  });

  it("names unreadable files and why, since whatever they hold may load", () => {
    const unreadable = [
      { path: ".claude/settings.json", kind: "unreadable" as const, entries: ["expected value at line 1 column 2"] },
      { path: ".claude/settings.local.json", kind: "unreadable" as const, entries: ["larger than 1048576 bytes"] },
    ];
    expect(warnings(unreadable, "untrusted").unreadable).toBe(
      "Ovra could not read .claude/settings.json (expected value at line 1 column 2) and .claude/settings.local.json (larger than 1048576 bytes); whatever they hold, hooks too, may load once you trust this project.",
    );
    expect(warnings(unreadable.slice(1), "changed").unreadable).toBe(
      "Ovra could not read the new .claude/settings.local.json (larger than 1048576 bytes); whatever it holds, hooks too, may load once you trust the changes.",
    );
  });

  it("names environment variables that redirect Claude or run code", () => {
    const risky = [
      "ANTHROPIC_BASE_URL",
      "ANTHROPIC_API_KEY",
      "CLAUDE_CODE_USE_BEDROCK",
      "OPENAI_BASE_URL",
      "HTTPS_PROXY",
      "https_proxy",
      "ALL_PROXY",
      "NODE_OPTIONS",
      "BASH_ENV",
      "ENV",
      "LD_PRELOAD",
      "DYLD_INSERT_LIBRARIES",
      "PATH",
      "Path",
      "PYTHONPATH",
      "PYTHONSTARTUP",
      "GIT_SSH",
      "GIT_SSH_COMMAND",
      "GIT_EXEC_PATH",
    ];
    expect(risky.filter((name) => !riskyEnv(name))).toEqual([]);
    const harmless = ["RUST_LOG", "NODE_ENV", "DEBUG", "GIT_AUTHOR_NAME", "MY_PATH_HINT", "ENVIRONMENT"];
    expect(harmless.filter(riskyEnv)).toEqual([]);
    const env = (entries: string[]) => [{ path: ".claude/settings.json", kind: "env" as const, entries }];
    expect(warnings(env(["ANTHROPIC_BASE_URL", "NODE_ENV", "NODE_OPTIONS"]), "untrusted").env).toBe(
      "Project settings set ANTHROPIC_BASE_URL and NODE_OPTIONS: once you trust this project, they can send your prompts and code elsewhere or run code.",
    );
    expect(warnings(env(["HTTPS_PROXY"]), "trusted").env).toBe(
      "Project settings set HTTPS_PROXY: it can send your prompts and code elsewhere or run code.",
    );
    expect(warnings(env(["PATH"]), "changed").env).toBe(
      "New project settings set PATH: once you trust the changes, it can send your prompts and code elsewhere or run code.",
    );
    expect(warnings(env(["NODE_ENV"]), "untrusted").env).toBeNull();
    // The core sends `NAME = value`, so a changed value is a new entry and warns again.
    expect(warnings(env(["ANTHROPIC_BASE_URL = https://attacker.example", "RUST_LOG = info"]), "changed").env).toBe(
      "New project settings set ANTHROPIC_BASE_URL: once you trust the changes, it can send your prompts and code elsewhere or run code.",
    );
    // Names the core cut from the list cannot be checked: said, not assumed harmless.
    expect(warnings(env(["NODE_ENV", "… and 3 more"]), "untrusted").env).toBe(
      "3 environment variables are not listed, so Ovra could not check them: read the settings files.",
    );
  });
});

describe("AgentSetup", () => {
  it("shows nothing until the first check answers, nor once Claude is ready", async () => {
    const core = new FakeAgentCore();
    const call = core.call.bind(core);
    let answer = () => {};
    const held = new Promise<void>((resolve) => (answer = resolve));
    core.call = (async (request) => {
      if (request.method === "agents.setup") await held;
      return call(request);
    }) as FakeAgentCore["call"];
    await show(core);
    expect(card()).toBeNull();
    answer();
    await settle();
    expect(card()).toBeNull();
  });

  it("opens from the start form's billing button, and hides the path field until Change", async () => {
    await show(new FakeAgentCore());
    button(target, "Claude · Claude plan")?.click();
    await settle();
    expect(card()?.textContent?.replace(/\s+/g, " ").trim()).toMatch(
      /^Claude 2.1.331 is ready · uses your Claude plan Change/,
    );
    expect(card()?.querySelector("#claude-path")).toBeNull();
    button(card(), "Change")?.click();
    flushSync();
    expect(card()?.querySelector("#claude-path")).not.toBeNull();
    expect(card()?.querySelector("[data-testid=claude-executable]")?.textContent).toBe("Using /usr/bin/claude");
    button(card(), "Cancel")?.click();
    flushSync();
    expect(card()?.querySelector("#claude-path")).toBeNull();
    // The same button closes it again.
    button(target, "Claude · Claude plan")?.click();
    flushSync();
    expect(card()).toBeNull();
  });

  it("says who pays for usage and warns about an API key in the environment", async () => {
    const core = new FakeAgentCore();
    await show(core);
    button(target, "Claude · Claude plan")?.click();
    await settle();
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
    expect(advice(keyed, "Claude").detail).toBeNull();
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
    expect(card()).toBeNull();
    expect(target.querySelector("[data-testid=claude-api-key]")?.textContent).toContain("not to your Claude plan");
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
    expect(card()).toBeNull();
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
    expect(testid("trust-unreadable")).toBe(
      "⚠ Ovra could not read .claude/settings.json (too large); whatever it holds, hooks too, may load once you trust this project.",
    );

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
    // Plain lines only: no counts of every kind, no raw rules (those are in the lists).
    expect(testid("trust-summary")).toBeNull();
    expect(testid("broad-rules")).toBe(
      "⚠ Once you trust this project, Claude will use 2 tools and commands without asking you, including any shell command.",
    );
    expect(testid("trust-runs-code")).toBe("⚠ Once you trust this project, 1 hook will run commands on your computer.");
    // No sandbox here (the fake reports none): said before the Trust button, not only in the details.
    expect(testid("trust-no-sandbox")).toContain("without a sandbox");
    // The lists wait behind "Review and trust…"; Trust is only at their bottom.
    expect(trustCard()?.querySelector("[data-testid=trust-details]")).toBeNull();
    expect(button(trustCard(), "Trust project")).toBeUndefined();
    await armed();
    button(trustCard(), "Review and trust…")?.click();
    await settle();
    // A click right as the lists appear is not a decision: nothing is sent.
    const trustButton = button(trustCard(), "Trust project");
    expect(trustButton?.closest("[data-testid=trust-details]")).not.toBeNull();
    expect(trustButton?.getAttribute("aria-disabled")).toBe("true");
    trustButton?.click();
    await settle();
    expect(core.sent("projects.trust")).toEqual([]);
    // ... and says why, until the button is ready.
    const wait = () => trustCard()?.querySelector("[data-testid=trust-wait]")?.textContent;
    expect(wait()).toBe("Ready in a moment — read the card first.");
    await armed();
    expect(wait()).toBe("");
    expect(trustButton?.getAttribute("aria-disabled")).toBe("false");
    trustButton?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params)).toEqual([{ id: "p1", trusted: true, fingerprint: "f1" }]);
    expect(workspace.project?.trusted).toBe(true);
    // The button went with the details: focus stays on the card.
    expect(document.activeElement?.id).toBe("trust-heading");
    // Trusted: one quiet line with the warning count in its Details button; the rest under it.
    expect(heading()).toBe("Project's own settings are on");
    expect(testid("trust-warning-count")).toBe("⚠ 2 warnings");
    expect(trustCard()?.querySelector("[data-testid=trust-warning-count]")?.getAttribute("title")).toBe(
      "2 warnings: see Details",
    );
    expect(button(trustCard(), "Stop trusting")).toBeUndefined();
    const toggle = trustCard()?.querySelector<HTMLButtonElement>("[aria-controls=trust-body]");
    expect(toggle?.textContent?.replace(/\s+/g, " ").trim()).toBe("⚠ 2 warnings Details");
    toggle?.click();
    await settle();
    // Open, the same button in the same heading row folds it again; the warnings stay on the card.
    expect(toggle?.textContent?.trim()).toBe("Hide");
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(toggle?.closest(".row")?.querySelector("h3")).not.toBeNull();
    expect(testid("trust-runs-code")).toBe("⚠ 1 hook runs commands on your computer.");
    // The lists wait one level deeper.
    expect(testid("trust-details")).toBeNull();
    button(trustCard(), "What's included")?.click();
    await settle();
    expect(testid("trust-details")).toContain("./check.sh");
    // Escape folds it all again, back to the card's heading.
    toggle?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
    await settle();
    expect(testid("trust-details")).toBeNull();
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement?.id).toBe("trust-heading");
    toggle?.click();
    await settle();
    button(trustCard(), "What's included")?.click();
    await settle();
    // Trusted, nothing to decide: only the risky groups start open.
    const open = [...(trustCard()?.querySelectorAll<HTMLDetailsElement>("details.group") ?? [])].map((g) => g.open);
    expect(open).toEqual([true, true, false]);
    toggle?.click();
    await settle();
    expect(testid("trust-warning-count")).toBe("⚠ 2 warnings");
    toggle?.click();
    await settle();
    button(trustCard(), "Stop trusting")?.click();
    await settle();
    expect(workspace.project?.trusted).toBe(false);
  });

  it("shows the full lists by kind, counted, behind Review and trust, long ones cut", async () => {
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
    const toggle = button(trustCard(), "Review and trust…");
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    toggle?.click();
    flushSync();
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    const groups = [...(trustCard()?.querySelectorAll("details.group") ?? [])] as HTMLDetailsElement[];
    // Deciding: every group is open, so Trust comes after all of it.
    expect(groups.map((g) => [g.dataset.kind, g.open])).toEqual([
      ["hooks", true],
      ["allow_rules", true],
    ]);
    expect(groups[0]?.textContent).toContain("runs code");
    expect(groups.map((g) => g.querySelector(".count")?.textContent)).toEqual(["1", "7"]);
    expect(testid("trust-details")).toContain("./check.sh");
    expect(testid("trust-language-servers")).toMatch(
      /Trusting also lets Ovra start code help \(language servers\).*runs the project's code.*Stop trusting stops it.*sessions started afterwards/s,
    );
    expect(groups[1]?.querySelectorAll("li")).toHaveLength(5);
    button(trustCard(), "+2 more")?.click();
    flushSync();
    expect(groups[1]?.querySelectorAll("li")).toHaveLength(7);
  });

  it("shows no card without Claude settings, until the review command asks for one", async () => {
    const core = new FakeAgentCore();
    const workspace = await show(core);
    expect(trustCard()).toBeNull();
    Element.prototype.scrollIntoView = vi.fn();
    await listCommands()
      .find((c) => c.id === "projects.review_trust")
      ?.run();
    await settle();
    expect(heading()).toBe("No Claude settings in this project");
    expect(testid("trust-language-servers")).toContain("starts code help (language servers)");
    expect(trustCard()?.querySelector("[role=note]")).toBeNull();
    expect(button(trustCard(), "Show details")).toBeUndefined();
    await armed();
    button(trustCard(), "Trust project")?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params)).toEqual([{ id: "p1", trusted: true, fingerprint: "f0" }]);
    expect(workspace.project?.trusted).toBe(true);
    // Trusted, still nothing of its own: the card goes again.
    expect(trustCard()).toBeNull();
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
    expect(testid("trust-runs-code")).toBe(
      "⚠ Once you trust the changes, 1 new hook will run commands on your computer.",
    );
    button(trustCard(), "Review and trust…")?.click();
    flushSync();
    expect(trustCard()?.querySelector(".added")?.textContent).toContain("PreToolUse: curl x | sh");
    expect(trustCard()?.querySelector(".removed")?.textContent).toContain("Stop: ./done.sh");
    // A change that lands while the user looks is refused, then shown.
    core.config = { ...core.config, fingerprint: "newer" };
    await armed();
    button(trustCard(), "Trust the new settings")?.click();
    await settle();
    expect(trustCard()?.querySelector("[role=alert]")?.textContent).toContain("review them again");
    // What the card shows changed: the button waits again before it counts a press.
    button(trustCard(), "Trust the new settings")?.click();
    await settle();
    expect(core.sent("projects.trust")).toHaveLength(1);
    await armed();
    button(trustCard(), "Trust the new settings")?.click();
    await settle();
    expect(core.sent("projects.trust").map((c) => c.params.fingerprint)).toEqual(["new", "newer"]);
    expect(heading()).toBe("Project's own settings are on");
    expect(testid("trust-details")).toBeNull();
  });

  it("rechecks when a session starts or the settings may have changed", async () => {
    const core = new FakeAgentCore();
    await show(core);
    core.status("s1", { state: "starting" });
    core.emit({ type: "review.changed", data: { project: "p1", id: "r1" } });
    await settle();
    expect(core.sent("agents.project_config")).toHaveLength(3);
    // The card is always shown, so an untrusted project's new settings come in too.
    core.config = { fingerprint: "f1", items: [{ path: ".claude/skills", kind: "skills", entries: ["a"] }] };
    core.emit({ type: "fs.changed", data: { project: "p1", paths: [".claude/skills/a/SKILL.md"] } });
    await settle();
    expect(heading()).toBe("This project has its own Claude settings");
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

  it("warns when only file contents changed, since no list names them", async () => {
    const core = new FakeAgentCore();
    const skill = { path: ".claude/skills", kind: "skills" as const, entries: ["release"] };
    core.trustedConfig.set("p1", { fingerprint: "old", items: [skill] });
    core.config = { fingerprint: "new", items: [skill] };
    const workspace = new Workspace();
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    app = mount(TestHost, { target, props: { transport: core, workspace } });
    await settle();
    expect(heading()).toBe("Project settings changed");
    const line = trustCard()?.querySelector("[data-testid=trust-summary]");
    expect(line?.classList.contains("warning")).toBe(true);
    expect(line?.textContent?.trim()).toMatch(/^⚠ The list is the same, but files under \.claude\/ changed/);
  });

  it("stops saying trusted once an approved review or a file changes the settings", async () => {
    const core = new FakeAgentCore();
    core.trustedConfig.set("p1", core.config);
    const workspace = await show(core);
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: true };
    await settle();
    expect(trustCard()).toBeNull();
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

  it("puts the review command on the heading, never on a Trust button, and ignores a held Enter", async () => {
    const core = new FakeAgentCore();
    core.config = {
      fingerprint: "f1",
      items: [{ path: ".claude/settings.json", kind: "broad_allow_rules", entries: ["Bash(*)"] }],
    };
    await show(core);
    await armed();
    // jsdom does not scroll.
    Element.prototype.scrollIntoView = vi.fn();
    const command = listCommands().find((c) => c.id === "projects.review_trust");
    await command?.run();
    await settle();
    expect(testid("trust-details")).not.toBeNull();
    expect(document.activeElement).toBe(trustCard()?.querySelector("#trust-heading"));
    // The next Enter (a double press in the palette) lands on the heading and trusts nothing.
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", key: "Enter", bubbles: true }));
    await settle();
    expect(core.sent("projects.trust")).toEqual([]);
    // Showing the details re-arms the buttons.
    expect(button(trustCard(), "Trust project")?.getAttribute("aria-disabled")).toBe("true");
    await armed();
    const trustButton = button(trustCard(), "Trust project");
    const press = (repeat: boolean) => {
      const event = new KeyboardEvent("keydown", {
        code: "Enter",
        key: "Enter",
        repeat,
        bubbles: true,
        cancelable: true,
      });
      trustButton?.dispatchEvent(event);
      return event.defaultPrevented;
    };
    // Auto-repeat would click the focused button again and again: it is cancelled.
    expect(press(true)).toBe(true);
    expect(press(false)).toBe(false);
  });

  it("warns about risky environment variables and keeps sandbox internals in a tooltip", async () => {
    const core = new FakeAgentCore();
    core.config = {
      fingerprint: "f1",
      items: [{ path: ".claude/settings.json", kind: "env", entries: ["ANTHROPIC_BASE_URL", "NODE_ENV"] }],
    };
    const call = core.call.bind(core);
    core.call = ((request: Parameters<typeof call>[0]) =>
      request.method === "lsp.status"
        ? Promise.resolve({
            data: { trusted: false, sandbox: "bubblewrap, Landlock, seccomp", servers: [], known: [] },
          })
        : call(request)) as typeof core.call;
    await show(core);
    await languageServers(core).show("p1");
    await settle();
    expect(testid("trust-risky-env")).toBe(
      "⚠ Project settings set ANTHROPIC_BASE_URL: once you trust this project, it can send your prompts and code elsewhere or run code.",
    );
    button(trustCard(), "Review and trust…")?.click();
    flushSync();
    const servers = trustCard()?.querySelector("[data-testid=trust-details] [data-testid=trust-language-servers]");
    expect(servers?.textContent).toContain("in the sandbox, kept to the project and without network");
    expect(servers?.textContent).not.toMatch(/bubblewrap|Landlock|seccomp/);
    expect(servers?.querySelector("[title]")?.getAttribute("title")).toBe("bubblewrap, Landlock, seccomp");
  });
});
