import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import { Workspace } from "../shell/workspace.svelte";
import { FakePolicyCore } from "./fake-policy";
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
  for (let i = 0; i < 10; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

let workspace: Workspace;

async function setup() {
  const core = new FakePolicyCore();
  core.rules.set("p1", [
    { tool: "Bash", match: "npm test", action: "auto" },
    { tool: "WebFetch", action: "deny" },
  ]);
  workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

const $ = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector);
const rules = () => [...target.querySelectorAll("[data-testid=policy-rule] .line")].map((el) => el.textContent?.trim());

function type(selector: string, text: string) {
  const input = $<HTMLInputElement | HTMLSelectElement>(selector);
  if (!input) throw new Error(`no ${selector}`);
  input.value = text;
  input.dispatchEvent(new Event(input instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
}

async function submit() {
  $<HTMLFormElement>("[data-testid=policy] form")?.requestSubmit();
  await settle();
}

describe("PolicyPanel", () => {
  it("opens from its command on the rules, deny first, with their caveats, and returns focus on Escape", async () => {
    await setup();
    const open = $<HTMLButtonElement>("[data-testid=open-policy]");
    open?.focus();
    await listCommands()
      .find((c) => c.id === "policy.toggle")
      ?.run();
    await settle();
    expect(document.activeElement?.id).toBe("policy-title");
    expect(rules()).toEqual([
      expect.stringMatching(/^WebFetch\s+Deny/),
      expect.stringMatching(/^Bash\(npm test\)\s+Auto/),
    ]);
    const notes = $("[data-testid=policy-caveat]")?.textContent ?? "";
    expect(notes).toMatch(/only in a sandboxed session whose network is limited\. Otherwise Ovra still asks/);
    expect(notes).toMatch(/npm test also runs .*the agent can edit those/);

    $("[data-testid=policy]")?.dispatchEvent(
      new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true }),
    );
    await settle();
    expect($("[data-testid=policy]")).toBeNull();
    expect(document.activeElement).toBe(open);
  });

  it("adds, edits and removes rules through policy.set", async () => {
    const core = await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    type("[data-testid=policy-tool]", "Bash");
    type("[data-testid=policy-match]", "git push");
    type("[data-testid=policy-action]", "deny");
    await submit();
    expect(core.rules.get("p1")?.[2]).toEqual({ tool: "Bash", match: "git push", action: "deny" });
    expect(rules()[1]).toMatch(/^Bash\(git push\)\s+Deny/);
    expect(target.textContent).toMatch(/Advisory: the same command written another way/);
    expect(document.activeElement).toBe($("[data-testid=policy-tool]"));

    $<HTMLButtonElement>("[aria-label='Edit WebFetch deny']")?.click();
    await settle();
    expect($<HTMLInputElement>("[data-testid=policy-tool]")?.value).toBe("WebFetch");
    type("[data-testid=policy-action]", "ask");
    await submit();
    expect(core.rules.get("p1")?.[1]).toEqual({ tool: "WebFetch", action: "ask" });

    $<HTMLButtonElement>("[aria-label='Remove Bash(npm test) auto']")?.click();
    await settle();
    expect(core.rules.get("p1")).toEqual([
      { tool: "WebFetch", action: "ask" },
      { tool: "Bash", match: "git push", action: "deny" },
    ]);
    expect(core.calls.filter((c) => c.method === "policy.set")).toHaveLength(3);
  });

  it("shows the core's refusal inline and keeps the form", async () => {
    const core = await setup();
    core.refuse.set("Grep", "Grep(src/**): only Bash and file tools take a match; leave it out to match every call");
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    type("[data-testid=policy-tool]", "Grep");
    type("[data-testid=policy-match]", "src/**");
    await submit();
    expect($("[data-testid=policy-error]")?.textContent).toBe(
      "Grep(src/**): only Bash and file tools take a match; leave it out to match every call",
    );
    expect($<HTMLInputElement>("[data-testid=policy-match]")?.value).toBe("src/**");
    expect(rules()).toHaveLength(2);
  });

  it("keeps what was typed when the same project's details refresh, and clears it for another", async () => {
    const core = await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    type("[data-testid=policy-tool]", "Bash");
    workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: 1, trusted: true };
    await settle();
    expect($<HTMLInputElement>("[data-testid=policy-tool]")?.value).toBe("Bash");
    expect(core.calls.filter((c) => c.method === "policy.get")).toHaveLength(1);
    workspace.project = { id: "p2", name: "other", root: "/other", last_opened_ms: null, trusted: false };
    await settle();
    expect($<HTMLInputElement>("[data-testid=policy-tool]")?.value).toBe("");
    expect($("[data-testid=policy-empty]")?.textContent).toMatch(/No rules yet/);
  });

  it("keeps Tab inside the dialog", async () => {
    await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    const tab = (shiftKey: boolean) =>
      $("[data-testid=policy]")?.dispatchEvent(
        new KeyboardEvent("keydown", { code: "Tab", key: "Tab", shiftKey, bubbles: true, cancelable: true }),
      );
    $<HTMLButtonElement>("button[type=submit]")?.focus();
    tab(false);
    expect(document.activeElement?.textContent?.trim()).toBe("Close");
    tab(true);
    expect(document.activeElement?.textContent?.trim()).toBe("Add rule");
  });

  it("reads the rules again when the policy changes elsewhere", async () => {
    const core = await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    core.rules.set("p1", [{ tool: "Write", match: "docs/**", action: "auto" }]);
    core.emit({ type: "policy.changed", data: { project: "p1" } });
    await settle();
    expect(rules()).toEqual([expect.stringMatching(/^Write\(docs\/\*\*\)\s+Auto/)]);
  });
});
