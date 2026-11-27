import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { SessionInfo } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAccessCore } from "./fake-access";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;
let workspace: Workspace;

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
  for (let i = 0; i < 16; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

async function open(core = new FakeAccessCore()) {
  workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace, pane: "keys" } });
  await settle();
  return core;
}

const $ = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector);

function type(selector: string, text: string) {
  const input = $<HTMLInputElement>(selector);
  if (!input) throw new Error(`no ${selector}`);
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function submit() {
  $("form[aria-label='Add a key']")?.dispatchEvent(new SubmitEvent("submit", { bubbles: true, cancelable: true }));
}

async function preset(id: string) {
  $<HTMLButtonElement>(`[data-preset=${id}]`)?.click();
  await settle();
}

function consent(to: string) {
  const select = $<HTMLSelectElement>("[data-testid=key-consent]");
  if (!select) throw new Error("no consent select");
  select.value = to;
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

const github = {
  name: "github",
  host: "api.github.com",
  header: "Authorization",
  prefix: "Bearer ",
  variable: "GH_TOKEN",
};

describe("KeysPane", () => {
  it("adds a GitHub key from its value alone and allows its host in the same step", async () => {
    const core = await open();
    expect($("[data-testid=keys-empty]")?.textContent).toContain("No keys");
    await preset("github");
    expect(document.activeElement).toBe($("[data-testid=key-value]"));
    expect($("form[aria-label='Add a key'] details")?.hasAttribute("open")).toBe(false);
    type("[data-testid=key-value]", "ghp_example");
    submit();
    await settle();
    expect(core.values.get("github")).toBe("ghp_example");
    expect(core.secrets).toEqual([github]);
    expect(core.hosts).toEqual([{ host: "api.github.com" }]);
    expect($("form[aria-label='Add a key']")).toBeNull();
    expect(target.textContent).not.toContain("ghp_example");
    expect($("[data-testid=key]")?.textContent).toContain("as $GH_TOKEN");
    expect($("[data-testid=key]")?.textContent).not.toContain("cannot reach");
  });

  it("keeps focus in the form while the keychain is checked, then moves it to the field", async () => {
    const core = new FakeAccessCore();
    let release = () => {};
    core.listing = new Promise((resolve) => {
      release = resolve;
    });
    await open(core);
    await preset("github");
    expect(document.activeElement).toBe($("[data-testid=add-title]"));
    release();
    await settle();
    expect(document.activeElement).toBe($("[data-testid=key-value]"));
  });

  it("moves focus to Check again when the keychain turns out unreachable", async () => {
    const core = new FakeAccessCore();
    core.store = { state: "unavailable", reason: "no Secret Service" };
    let release = () => {};
    core.listing = new Promise((resolve) => {
      release = resolve;
    });
    await open(core);
    await preset("github");
    release();
    await settle();
    expect(document.activeElement).toBe($("[data-testid=store-retry]"));
  });

  it("asks for every field for another API, and the value first", async () => {
    const core = await open();
    await preset("other");
    expect($("form[aria-label='Add a key'] details")?.hasAttribute("open")).toBe(true);
    submit();
    await settle();
    expect($("[role=alert]")?.textContent).toBe("Paste the key.");
    type("[data-testid=key-value]", "v");
    type("[data-testid=key-name]", "deploy");
    type("[data-testid=key-host]", "API.Example.com");
    submit();
    await settle();
    expect(core.secrets).toEqual([
      { name: "deploy", host: "api.example.com", header: "Authorization", prefix: "Bearer " },
    ]);
  });

  it("keeps each key's consent as UseKey rules and drops them with the key", async () => {
    const core = new FakeAccessCore();
    core.secrets = [github];
    core.hosts = [{ host: "api.github.com" }];
    const other = { tool: "Bash", match: "npm test", action: "auto" as const };
    core.rules = [other];
    await open(core);
    expect($<HTMLSelectElement>("[data-testid=key-consent]")?.value).toBe("ask");
    consent("always");
    await settle();
    expect(core.rules).toEqual([other, { tool: "UseKey", match: "github", action: "auto" }]);
    expect($<HTMLSelectElement>("[data-testid=key-consent]")?.value).toBe("always");
    consent("off");
    await settle();
    expect(core.rules).toEqual([other, { tool: "UseKey", match: "github", action: "deny" }]);
    consent("ask");
    await settle();
    expect(core.rules).toEqual([other]);
    consent("always");
    await settle();
    $<HTMLButtonElement>("[aria-label='Remove key github']")?.click();
    await settle();
    expect(core.secrets).toEqual([]);
    expect(core.rules).toEqual([other]);
  });

  it("shows a project-wide UseKey deny as Off for every key", async () => {
    const core = new FakeAccessCore();
    core.secrets = [github];
    core.rules = [{ tool: "UseKey", action: "deny" }];
    await open(core);
    expect($<HTMLSelectElement>("[data-testid=key-consent]")?.value).toBe("off");
  });

  it("says why keys do nothing for agents without a sandbox", async () => {
    const core = new FakeAccessCore();
    const bare: SessionInfo = {
      id: "s1",
      project: "p1",
      agent: "claude",
      mode: "direct",
      status: { state: "idle" },
      title: "fix it",
      usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
      resumable: false,
      billing: null,
      sandbox: null,
      sandbox_reported: true,
      account: null,
    };
    core.sessions = [bare];
    await open(core);
    expect($("[data-testid=keys-unsandboxed]")?.textContent).toContain("Ovra adds no keys for them");
  });

  it("says the keychain is unreachable instead of a form, and forgets a typed key", async () => {
    const core = new FakeAccessCore();
    core.store = { state: "unavailable", reason: "zbus error" };
    await open(core);
    await preset("github");
    expect($("form[aria-label='Add a key']")).toBeNull();
    expect(document.activeElement).toBe($("[data-testid=store-retry]"));
    core.store = { state: "available", name: "Secret Service" };
    $<HTMLButtonElement>("[data-testid=store-retry]")?.click();
    await settle();
    type("[data-testid=key-value]", "typed");
    expect($("[data-testid=store-available]")?.textContent).toContain("Kept in Secret Service");
    workspace.project = { id: "p2", name: "other", root: "/other", last_opened_ms: null, trusted: false };
    await settle();
    await preset("github");
    expect($<HTMLInputElement>("[data-testid=key-value]")?.value).toBe("");
    const input = $<HTMLInputElement>("[data-testid=key-value]");
    expect(input?.type).toBe("text");
    expect(input?.autocomplete).toBe("off");
  });
});
