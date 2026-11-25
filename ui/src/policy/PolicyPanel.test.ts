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

async function setup() {
  const core = new FakePolicyCore();
  core.rules.set("p1", [
    { tool: "Bash", match: "npm test", action: "auto" },
    { tool: "WebFetch", action: "deny" },
  ]);
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

const $ = <T extends HTMLElement>(selector: string) => target.querySelector<T>(selector);
const rules = () => [...target.querySelectorAll("[data-testid=policy-rule] .line")].map((el) => el.textContent?.trim());

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
