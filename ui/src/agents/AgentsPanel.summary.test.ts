import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { settings } from "../settings/panes.svelte";
import { settingsWindow } from "../settings/window.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
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
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

async function setup(core = new FakeAgentCore()) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

describe("AgentsPanel summary", () => {
  it("shows the agent's three-line summary once its turn is over", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix it", { state: "working" });
    core.transcripts.set("s1", [
      { kind: "user_text", text: "Fix it", from: "person" },
      { kind: "assistant_text", text: "Fixed.\n\nDone: fixed the typo\nChecked: just test passed\nLeft: Nothing" },
    ]);
    await setup(core);
    [...target.querySelectorAll<HTMLButtonElement>(".row-button")]
      .find((b) => b.textContent?.includes("Fix it"))
      ?.click();
    await settle();
    expect(target.querySelector("[data-testid=task-summary]")).toBeNull();
    core.status("s1", { state: "idle" });
    await settle();
    const lines = [...target.querySelectorAll("[data-testid=task-summary] .row")].map((r) =>
      r.textContent?.replace(/\s+/g, " ").trim(),
    );
    expect(lines).toEqual(["Done fixed the typo", "Checked just test passed", "Left Nothing"]);
  });
});

describe("AgentsPanel things to check", () => {
  const badge = () => target.querySelector<HTMLButtonElement>("[data-testid=agents-attention]");
  const items = () =>
    [...target.querySelectorAll("[role=menu] [role=menuitem]")].map((i) => i.textContent?.replace(/\s+/g, " ").trim());

  it("shows nothing while the check finds nothing and no decision waits", async () => {
    await setup();
    expect(badge()).toBeNull();
    expect(target.querySelector("[data-testid=agents-empty]")?.textContent).toBe("Tasks you give Claude show up here.");
  });

  it("counts a setup problem and a waiting trust decision, and leads to each", async () => {
    const core = new FakeAgentCore();
    core.setupReport = {
      agents: [{ id: "claude", name: "Claude", setup: { ...core.setup } }],
      git: { state: "checked", found: { installed: false, version: null } },
      gh: { state: "skipped" },
      sandbox: { state: "unavailable", reason: "install bubblewrap (bwrap) to sandbox agents" },
    };
    core.config = {
      fingerprint: "f1",
      items: [{ path: ".claude/settings.json", kind: "broad_allow_rules", entries: ["Bash(*)"] }],
    };
    await setup(core);
    expect(core.sent("setup.check").map((c) => c.params)).toEqual([{ project: "p1" }]);
    // No card in the list: the decision waits behind the count.
    expect(target.querySelector("[data-testid=project-trust]")).toBeNull();
    expect(badge()?.getAttribute("aria-label")).toBe("2 things to check");
    badge()?.click();
    await settle();
    expect(items()).toEqual([
      "Setup check found 2 problems Check setup",
      "Project settings are off 1 warning · Review and trust…",
    ]);
    expect(document.activeElement?.getAttribute("data-testid")).toBe("attention-setup");
    target.querySelector<HTMLButtonElement>("[data-testid=attention-trust]")?.click();
    await settle();
    // The card opens with its warnings; Trust still waits at the bottom of its lists.
    expect(target.querySelector("[data-testid=project-trust] h3")?.textContent).toBe(
      "This project has its own Claude settings",
    );
    expect(target.querySelector("[data-testid=broad-rules]")).not.toBeNull();
    expect(core.sent("projects.trust")).toEqual([]);
    badge()?.click();
    await settle();
    target.querySelector<HTMLButtonElement>("[data-testid=attention-setup]")?.click();
    flushSync();
    expect(settings.pane).toBe("setup");
    expect(settingsWindow.open).toBe(true);
    settingsWindow.close(false);
  });
});
