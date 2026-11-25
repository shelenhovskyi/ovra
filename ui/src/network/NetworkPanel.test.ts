import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { SessionInfo } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAccessCore } from "./fake-access";
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
  for (let i = 0; i < 16; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

let workspace: Workspace;

const session = (id: string, sandbox: SessionInfo["sandbox"]): SessionInfo => ({
  id,
  project: "p1",
  agent: "claude",
  mode: "direct",
  status: { state: "idle" },
  title: "fix it",
  usage: { input_tokens: 0, output_tokens: 0, cost_micro_usd: null },
  resumable: false,
  billing: null,
  sandbox,
  sandbox_reported: true,
  account: null,
});

async function open(core = new FakeAccessCore()) {
  workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
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

function submit(form: HTMLFormElement | null | undefined) {
  form?.dispatchEvent(new SubmitEvent("submit", { bubbles: true, cancelable: true }));
}

function addSecret() {
  $<HTMLButtonElement>("[data-testid=add-secret]")?.click();
  flushSync();
}

const hosts = () => [...target.querySelectorAll("[data-testid=allowed-host] code")].map((el) => el.textContent);

describe("NetworkPanel", () => {
  it("adds and removes project hosts and warns about broad ones", async () => {
    const core = await open();
    expect($("[data-testid=hosts-empty]")?.textContent).toContain("No hosts added");
    type("[data-testid=add-host]", "Gist.GitHub.com");
    submit($("[data-testid=add-host]")?.closest("form"));
    await settle();
    type("[data-testid=add-host]", "registry.npmjs.org:8443");
    submit($("[data-testid=add-host]")?.closest("form"));
    await settle();
    expect(core.hosts).toEqual([{ host: "gist.github.com" }, { host: "registry.npmjs.org", port: 8443 }]);
    expect(hosts()).toEqual(["gist.github.com", "registry.npmjs.org:8443"]);
    expect(target.querySelectorAll("[data-testid=broad-host]")).toHaveLength(1);

    $<HTMLButtonElement>("[aria-label='Remove gist.github.com']")?.click();
    await settle();
    expect(core.hosts).toEqual([{ host: "registry.npmjs.org", port: 8443 }]);
  });

  it("explains a host it cannot take without asking the core", async () => {
    const core = await open();
    type("[data-testid=add-host]", "https://example.com/x");
    submit($("[data-testid=add-host]")?.closest("form"));
    await settle();
    expect($("[role=alert]")?.textContent).toContain("without a scheme");
    expect(core.calls.some((c) => c.method === "netproxy.set_allowlist")).toBe(false);
  });

  it("sends a secret's value to the core once, clears it and offers to allow its host", async () => {
    const core = await open();
    addSecret();
    type("[data-testid=secret-name]", "github");
    type("[data-testid=secret-host]", "api.github.com");
    type("[data-testid=secret-value]", "ghp_example");
    submit($<HTMLFormElement>("form[aria-label='Add a secret']"));
    await settle();
    expect(core.values.get("github")).toBe("ghp_example");
    expect(core.secrets).toEqual([
      { name: "github", host: "api.github.com", header: "Authorization", prefix: "Bearer " },
    ]);
    expect($<HTMLInputElement>("[data-testid=secret-value]")?.value).toBe("");
    expect(target.textContent).not.toContain("ghp_example");
    expect($("[data-testid=secret]")?.textContent).toContain("Agents cannot reach this host yet.");

    [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Allow it")?.click();
    await settle();
    expect(core.hosts).toEqual([{ host: "api.github.com" }]);
    expect($("[data-testid=secret]")?.textContent).not.toContain("cannot reach");

    $<HTMLButtonElement>("[aria-label='Remove secret github']")?.click();
    await settle();
    expect(core.secrets).toEqual([]);
  });

  it("says the keychain is unreachable only once a secret is being added, and why", async () => {
    const core = new FakeAccessCore();
    core.store = { state: "unavailable", reason: "zbus error: Failed to connect (os error 2)" };
    await open(core);
    expect($("[data-testid=store-unavailable]")).toBeNull();
    addSecret();
    const note = $("[data-testid=store-unavailable]");
    expect(note?.querySelector("p")?.textContent?.trim()).toMatch(/^Secrets can't be saved: the system keychain/);
    expect(note?.querySelector("p")?.textContent).not.toContain("zbus");
    expect($("details [data-testid=store-reason]")?.textContent).toBe("zbus error: Failed to connect (os error 2)");
    expect($("form[aria-label='Add a secret']")).toBeNull();

    core.store = { state: "available", name: "Secret Service" };
    $<HTMLButtonElement>("[data-testid=store-retry]")?.click();
    await settle();
    expect($("[data-testid=store-unavailable]")).toBeNull();
    expect($<HTMLFieldSetElement>("form[aria-label='Add a secret'] fieldset")?.disabled).toBe(false);
    expect($("[data-testid=secret-off]")).toBeNull();
    expect($("[data-testid=store-available]")?.textContent).toContain("Kept in Secret Service");
  });

  it("follows changes made elsewhere and forgets a typed token when the pane closes", async () => {
    const core = await open();
    core.hosts = [{ host: "pypi.org" }];
    core.emit({ type: "netproxy.changed", data: { project: "p1" } });
    await settle();
    expect(hosts()).toEqual(["pypi.org"]);

    addSecret();
    type("[data-testid=secret-value]", "typed");
    if (app) unmount(app);
    app = mount(TestHost, { target, props: { transport: core, workspace } });
    await settle();
    addSecret();
    expect($<HTMLInputElement>("[data-testid=secret-value]")?.value).toBe("");
  });

  it("never replaces the hosts with a partial list when they could not be read", async () => {
    const core = new FakeAccessCore();
    core.hosts = [{ host: "pypi.org" }];
    core.refuse.set("netproxy.get_allowlist", "core went away");
    await open(core);
    expect($("[role=alert]")?.textContent).toContain("core went away");
    expect($("[data-testid=hosts-empty]")).toBeNull();
    type("[data-testid=add-host]", "example.com");
    submit($("[data-testid=add-host]")?.closest("form"));
    await settle();
    expect(core.calls.some((c) => c.method === "netproxy.set_allowlist")).toBe(false);
    expect(core.hosts).toEqual([{ host: "pypi.org" }]);
  });

  it("says No sandbox in the session's words when the project's agents run without one", async () => {
    const core = new FakeAccessCore();
    core.sessions = [session("s1", { name: "bubblewrap", network: "proxy", gaps: [] })];
    await open(core);
    expect($("[data-testid=unsandboxed]")).toBeNull();
    const says =
      "No sandbox: Claude can change anything you can: these hosts don't limit it, and it can read stored secrets.";
    for (const s2 of [session("s2", null), { ...session("s2", null), status: { state: "stopped" } as const }]) {
      unmount(app ?? {});
      app = null;
      // A live one, and an ended one: the next session most likely runs the same way.
      core.sessions = [session("s1", { name: "bubblewrap", network: "proxy", gaps: [] }), s2];
      await open(core);
      expect($("[data-testid=unsandboxed]")?.textContent?.trim()).toBe(says);
    }
    unmount(app ?? {});
    app = null;
    const failing = new FakeAccessCore();
    failing.refuse.set("agents.list", "core went away");
    await open(failing);
    expect($("[data-testid=unsandboxed]")?.textContent).toContain("Could not check");
  });

  it("lists what the sandbox of the project's live sessions does not cover, each once", async () => {
    const core = new FakeAccessCore();
    core.sessions = [session("s1", { name: "bubblewrap", network: "proxy", gaps: [] })];
    await open(core);
    expect($("[data-testid=sandbox-gaps]")).toBeNull();
    unmount(app ?? {});
    app = null;
    const boxed = { name: "fake sandbox", network: "proxy" as const };
    core.sessions = [
      session("s1", { ...boxed, gaps: ["DNS lookups", "loopback services"] }),
      session("s2", { ...boxed, gaps: ["DNS lookups", "shared folders"] }),
    ];
    await open(core);
    expect($("[data-testid=sandbox-gaps]")?.textContent).toBe(
      "The sandbox also does not cover: DNS lookups, loopback services, shared folders.",
    );
    expect($("[data-testid=sandbox-gaps]")?.classList.contains("muted")).toBe(true);
  });

  it("clears what was typed when another project opens", async () => {
    await open();
    addSecret();
    type("[data-testid=secret-value]", "for-p1");
    type("[data-testid=secret-name]", "github");
    workspace.project = { id: "p2", name: "other", root: "/other", last_opened_ms: null, trusted: false };
    await settle();
    addSecret();
    expect($<HTMLInputElement>("[data-testid=secret-value]")?.value).toBe("");
    expect($<HTMLInputElement>("[data-testid=secret-name]")?.value).toBe("");
  });

  it("does not offer the token to the browser's password store", async () => {
    await open();
    addSecret();
    const input = $<HTMLInputElement>("[data-testid=secret-value]");
    expect(input?.type).toBe("text");
    expect(input?.autocomplete).toBe("off");
  });
});
