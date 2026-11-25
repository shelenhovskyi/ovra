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
    const notes = [...target.querySelectorAll("[data-testid=policy-caveat]")].map((el) => el.textContent).join(" ");
    expect(notes).toMatch(/Only this tool is refused; others such as Bash can do the same/);
    expect(notes).toMatch(
      /only to an agent in the sandbox with limited network access\. Otherwise the call is decided as if this rule were not there/,
    );
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

  it("says once, without asking the core, that a tool takes no match", async () => {
    const core = await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    type("[data-testid=policy-tool]", "WebFetch");
    flushSync();
    const matchField = $<HTMLInputElement>("[data-testid=policy-match]");
    expect(matchField?.disabled).toBe(true);
    expect($("#policy-match-hint")?.textContent).toBe("WebFetch takes no match: the rule covers every call.");
    type("[data-testid=policy-tool]", "");
    flushSync();
    type("[data-testid=policy-match]", "src/**");
    type("[data-testid=policy-tool]", "Grep");
    await submit();
    expect(core.calls.filter((c) => c.method === "policy.set")).toHaveLength(0);
    expect(target.querySelectorAll("[role=alert]")).toHaveLength(1);
    expect($("[data-testid=policy-error]")?.textContent).toMatch(
      /^Grep takes no match: a rule for it covers every call/,
    );
    expect($("#policy-match-hint")).toBeNull();
    expect(document.activeElement).toBe(matchField);
    expect(matchField?.getAttribute("aria-invalid")).toBe("true");
    expect(matchField?.getAttribute("aria-describedby")).toBe("policy-error");
  });

  it("fixes a tool name's case and shows the core's refusal in plain words", async () => {
    const core = await setup();
    core.refuse.set("Bash", "Bash(rm -rf): the match is empty; leave it out to match every call");
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    type("[data-testid=policy-tool]", "bash");
    flushSync();
    expect($("#policy-tool-warning")?.textContent).toBe("Saved as Bash: tool names are case-sensitive.");
    type("[data-testid=policy-match]", "rm -rf");
    await submit();
    expect($("[data-testid=policy-error]")?.textContent).toBe("The match is empty; leave it out to match every call");
    const toolField = $<HTMLInputElement>("[data-testid=policy-tool]");
    expect(toolField?.getAttribute("aria-invalid")).toBe("true");
    expect(toolField?.getAttribute("aria-describedby")).toBe("policy-error policy-tool-warning");
    core.refuse.clear();
    await submit();
    expect(core.rules.get("p1")?.[2]).toEqual({ tool: "Bash", match: "rm -rf", action: "ask" });
  });

  it("keeps an edit whose rule changed elsewhere as a new rule instead of overwriting", async () => {
    const core = await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    $<HTMLButtonElement>("[aria-label='Edit WebFetch deny']")?.click();
    await settle();
    core.rules.set("p1", [
      { tool: "Bash", match: "npm test", action: "auto" },
      { tool: "Grep", action: "deny" },
    ]);
    core.emit({ type: "policy.changed", data: { project: "p1" } });
    await settle();
    type("[data-testid=policy-action]", "ask");
    await submit();
    expect($("[data-testid=policy-error]")?.textContent).toMatch(/changed elsewhere\. What you typed is kept/);
    expect(core.calls.filter((c) => c.method === "policy.set")).toHaveLength(0);
    expect($<HTMLInputElement>("[data-testid=policy-tool]")?.value).toBe("WebFetch");
    expect($("button[type=submit]")?.textContent?.trim()).toBe("Add rule");
    await submit();
    expect(core.rules.get("p1")).toEqual([
      { tool: "Bash", match: "npm test", action: "auto" },
      { tool: "Grep", action: "deny" },
      { tool: "WebFetch", action: "ask" },
    ]);
  });

  it("keeps editing the same rule when another one is removed", async () => {
    const core = await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    $<HTMLButtonElement>("[aria-label='Edit WebFetch deny']")?.click();
    await settle();
    const remove = $<HTMLButtonElement>("[aria-label='Remove Bash(npm test) auto']");
    expect(remove?.getAttribute("aria-describedby")).toBe("policy-notes-0");
    remove?.click();
    await settle();
    expect($("button[type=submit]")?.textContent?.trim()).toBe("Save rule");
    type("[data-testid=policy-action]", "ask");
    await submit();
    expect(core.rules.get("p1")).toEqual([{ tool: "WebFetch", action: "ask" }]);
  });

  it("adopts a policy changed elsewhere instead of dropping it, and keeps what was typed", async () => {
    const core = await setup();
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    // Another window adds a rule; its event has not arrived yet.
    core.rules.set("p1", [...(core.rules.get("p1") ?? []), { tool: "Read", match: "secrets/**", action: "deny" }]);
    type("[data-testid=policy-tool]", "Bash");
    type("[data-testid=policy-match]", "git push");
    await submit();
    expect($("[data-testid=policy-error]")?.textContent).toMatch(/changed elsewhere in the meantime/);
    expect(core.rules.get("p1")).toHaveLength(3);
    expect(rules()).toContainEqual(expect.stringMatching(/^Read\(secrets\/\*\*\)\s+Deny/));
    expect($<HTMLInputElement>("[data-testid=policy-match]")?.value).toBe("git push");
    await submit();
    expect(core.rules.get("p1")?.[3]).toEqual({ tool: "Bash", match: "git push", action: "ask" });
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

  it("keeps Tab inside the dialog while the form waits for the rules", async () => {
    const core = await setup();
    core.hold = "policy.get";
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    expect($("[data-testid=policy-empty]")?.textContent).toBe("Loading…");
    expect($<HTMLInputElement>("[data-testid=policy-tool]")?.matches(":disabled")).toBe(true);
    const focused: (string | undefined)[] = [];
    for (const shiftKey of [false, false, true]) {
      $("[data-testid=policy]")?.dispatchEvent(
        new KeyboardEvent("keydown", { code: "Tab", key: "Tab", shiftKey, bubbles: true, cancelable: true }),
      );
      focused.push(document.activeElement?.textContent?.trim());
    }
    // Only Close and the "How rules combine" disclosure: the disabled form is skipped.
    expect(focused).toEqual(["Close", "Close", "How rules combine"]);
    core.release();
  });

  it("offers to try again when the rules cannot be read", async () => {
    const core = await setup();
    core.readFails = "the core is not reachable";
    $<HTMLButtonElement>("[data-testid=open-policy]")?.click();
    await settle();
    expect($("[role=alert]")?.textContent).toMatch(/Could not read the project's policy: the core is not reachable/);
    core.readFails = null;
    $<HTMLButtonElement>("[data-testid=policy-retry]")?.click();
    await settle();
    expect(rules()).toHaveLength(2);
    expect($("[data-testid=policy-retry]")).toBeNull();
  });

  it("closes an open list of suggestions before the dialog, and returns focus to its button", async () => {
    const core = await setup();
    document.body.focus();
    await listCommands()
      .find((c) => c.id === "policy.toggle")
      ?.run();
    await settle();
    type("[data-testid=policy-tool]", "Ba");
    const pressEscape = () =>
      $("[data-testid=policy-tool]")?.dispatchEvent(
        new KeyboardEvent("keydown", { code: "Escape", key: "Escape", bubbles: true, cancelable: true }),
      );
    pressEscape();
    await settle();
    expect($("[data-testid=policy]")).not.toBeNull();
    pressEscape();
    await settle();
    expect($("[data-testid=policy]")).toBeNull();
    expect(document.activeElement).toBe($("[data-testid=open-policy]"));
    // A closed dialog no longer follows the policy.
    core.emit({ type: "policy.changed", data: { project: "p1" } });
    await settle();
    expect(core.calls.filter((c) => c.method === "policy.get")).toHaveLength(1);
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
