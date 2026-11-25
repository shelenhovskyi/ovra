import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
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

async function open(core = new FakeAccessCore()) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  target.querySelector<HTMLButtonElement>("[data-testid=open-network]")?.click();
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

  it("disables the form when the keychain is unavailable and shows why", async () => {
    const core = new FakeAccessCore();
    core.store = { state: "unavailable", reason: "no Secret Service is running" };
    await open(core);
    expect($("[data-testid=store-unavailable]")?.textContent).toContain("no Secret Service is running");
    expect($<HTMLFieldSetElement>("form[aria-label='Add a secret'] fieldset")?.disabled).toBe(true);
  });

  it("follows changes made elsewhere and closes on Escape", async () => {
    const core = await open();
    core.hosts = [{ host: "pypi.org" }];
    core.emit({ type: "netproxy.changed", data: { project: "p1" } });
    await settle();
    expect(hosts()).toEqual(["pypi.org"]);

    type("[data-testid=secret-value]", "typed");
    $("[data-testid=network]")?.dispatchEvent(
      new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true }),
    );
    await settle();
    expect($("[data-testid=network]")).toBeNull();
    $<HTMLButtonElement>("[data-testid=open-network]")?.click();
    await settle();
    expect($<HTMLInputElement>("[data-testid=secret-value]")?.value).toBe("");
  });
});
