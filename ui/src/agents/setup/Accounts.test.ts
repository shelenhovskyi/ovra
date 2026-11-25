import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AccountState } from "../../lib/protocol";
import { CoreError } from "../../lib/transport";
import { Workspace } from "../../shell/workspace.svelte";
import { FakeAgentCore } from "../fake-core";
import TestHost from "../TestHost.svelte";
import { accountStatus, nameFromFolder, signInCommand } from "./account-texts";

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
}

const accounts = () => target.querySelector<HTMLElement>("[data-testid=claude-accounts]");
const picker = () => target.querySelector<HTMLSelectElement>("[data-testid=account-picker] select");
const warning = () => target.querySelector("[data-testid=account-picker-warning]")?.textContent;
const summary = () => target.querySelector("[data-testid=claude-accounts-summary]")?.textContent;
const button = (label: string) => [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === label);

function change(select: HTMLSelectElement | null, value: string) {
  if (!select) return;
  select.value = value;
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

function type(selector: string, value: string) {
  const field = target.querySelector<HTMLInputElement>(selector);
  expect(field, selector).not.toBeNull();
  if (!field) return;
  field.value = value;
  field.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

function work(state: AccountState["setup"]["state"] = { state: "ready" }): AccountState {
  return {
    account: { id: "account-1", name: "Work", config_dir: "/Users/me/.claude-work" },
    setup: {
      path: null,
      executable: "/usr/bin/claude",
      version: "2.1.331",
      state,
      billing: state.state === "ready" ? { kind: "api_key" } : null,
      api_key_in_env: false,
    },
  };
}

describe("account texts", () => {
  it("say how each account stands and how to sign in to it", () => {
    expect(accountStatus(work())).toEqual({ text: "Signed in, billed to an API key", ok: true });
    expect(accountStatus(work({ state: "signed_out" }))).toMatchObject({
      ok: false,
      command: "CLAUDE_CONFIG_DIR=/Users/me/.claude-work claude auth login",
    });
    expect(signInCommand(null)).toBe("claude auth login");
    expect(signInCommand("/Users/me/My Claude")).toBe("CLAUDE_CONFIG_DIR='/Users/me/My Claude' claude auth login");
  });

  it("name a found folder after what follows .claude", () => {
    expect(nameFromFolder("/Users/me/.claude-work")).toBe("Work");
    expect(nameFromFolder("/home/me/.claude_personal/")).toBe("Personal");
    expect(nameFromFolder("/home/me/.claude")).toBe("Claude");
  });
});

describe("accounts in the agents panel", () => {
  it("offers only a way to add one while there is a single account", async () => {
    await show(new FakeAgentCore());
    // The ready card is one line: the way to add one shows once it is opened.
    expect(accounts()).toBeNull();
    button("Change")?.click();
    await settle();
    expect(accounts()?.textContent).toContain("Use another Claude account");
    expect(target.querySelectorAll("[data-testid=claude-account]")).toHaveLength(0);
    expect(picker()).toBeNull();
  });

  it("adds a found folder in one click and lets the next session use it", async () => {
    const core = new FakeAgentCore();
    core.found = ["/Users/me/.claude-work"];
    await show(core);
    button("Add as Work")?.click();
    await settle();
    expect(core.sent("agents.save_account").map((c) => c.params.account)).toEqual([
      { id: "", name: "Work", config_dir: "/Users/me/.claude-work" },
    ]);
    expect(summary()).toBe("Accounts: Default, Work");
    expect(target.querySelector("[data-testid=claude-account-found]")).toBeNull();

    // The picker starts at the project's account; a choice becomes the project's.
    const select = picker();
    expect(select).not.toBeNull();
    expect(select?.value).toBe("default");
    change(select, "account-1");
    await settle();
    expect(core.sent("agents.set_project_account").map((c) => c.params)).toEqual([
      { project: "p1", account: "account-1" },
    ]);
    const prompt = target.querySelector<HTMLTextAreaElement>("#agent-prompt");
    expect(prompt).not.toBeNull();
    if (prompt) {
      prompt.value = "Fix it";
      prompt.dispatchEvent(new Event("input", { bubbles: true }));
    }
    await settle();
    button("Start")?.click();
    await settle();
    expect(core.sent("agents.start").map((c) => c.params.account)).toEqual(["account-1"]);
  });

  it("shows the sign-in command of a signed-out account and removes it", async () => {
    const core = new FakeAgentCore();
    core.accounts = [...core.accounts, work({ state: "signed_out" })];
    core.projectAccounts.set("p1", "account-1");
    await show(core);
    expect(summary()).toBe("Accounts: Default, Work (not signed in)");
    // The project's account is signed out: a new task starts with a signed-in one.
    expect(picker()?.value).toBe("default");
    button("Manage")?.click();
    await settle();
    const status = [...target.querySelectorAll("[data-testid=claude-account-status]")].map((s) => s.textContent);
    expect(status[1]).toContain("Not signed in");
    expect(accounts()?.querySelector("code")?.textContent).toBe(
      "CLAUDE_CONFIG_DIR=/Users/me/.claude-work claude auth login",
    );
    target.querySelector<HTMLButtonElement>("[aria-label='Remove Work']")?.click();
    await settle();
    expect(core.sent("agents.remove_account").map((c) => c.params.id)).toEqual(["account-1"]);
    expect(target.querySelectorAll("[data-testid=claude-account]")).toHaveLength(1);
    expect(picker()).toBeNull();
  });

  it("adds a typed folder, says why the core refuses one, and renames", async () => {
    const core = new FakeAgentCore();
    await show(core);
    button("Change")?.click();
    await settle();
    button("Use another Claude account")?.click();
    await settle();
    type("#account-name", "Work");
    type("#account-folder", "~/.claude-work");
    core.failNext.set(
      "agents.save_account",
      new CoreError({ code: "bad_request", message: "the configuration folder must be a full path" }),
    );
    button("Add account")?.click();
    await settle();
    expect(accounts()?.querySelector("[role=alert]")?.textContent).toContain("full path");
    type("#account-folder", "/Users/me/.claude-work");
    button("Add account")?.click();
    await settle();
    expect(core.sent("agents.save_account").at(-1)?.params.account.config_dir).toBe("/Users/me/.claude-work");
    expect(summary()).toBe("Accounts: Default, Work");
    button("Manage")?.click();
    await settle();
    expect(target.querySelectorAll("[data-testid=claude-account]")).toHaveLength(2);

    target.querySelectorAll<HTMLButtonElement>("[data-testid=claude-account] button.link")[0]?.click();
    await settle();
    type("#account-rename", "Personal");
    button("Save")?.click();
    await settle();
    expect(core.sent("agents.save_account").at(-1)?.params.account).toEqual({
      id: "default",
      name: "Personal",
      config_dir: null,
    });
  });

  it("says in the picker which account bills a key and which is signed out", async () => {
    const core = new FakeAgentCore();
    const out = work({ state: "signed_out" });
    out.account = { id: "account-2", name: "Old", config_dir: "/Users/me/.claude-old" };
    core.accounts = [...core.accounts, work(), out];
    await show(core);
    const options = [...(picker()?.options ?? [])].map((o) => o.textContent);
    expect(options).toEqual(["Default", "Work (API key)", "Old (not signed in)"]);
    expect(warning()).toBeUndefined();
    change(picker(), "account-1");
    await settle();
    expect(warning()).toBe("Work bills an API key, at API prices.");
    // The start form says it when an account is signed out.
    change(picker(), "account-2");
    await settle();
    expect(warning()).toBeUndefined();
  });

  it("names the account that pays for a session", async () => {
    const core = new FakeAgentCore();
    core.accounts = [...core.accounts, work()];
    const info = core.addSession("p1", "fix", { state: "idle" });
    Object.assign(info, { account: "account-1", billing: { kind: "api_key" } });
    await show(core);
    expect(target.querySelector("[data-testid=agent-billing]")?.textContent).toContain("(Work account)");
  });
});
