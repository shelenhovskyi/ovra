import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { settings } from "../settings/panes.svelte";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
import { STARTERS } from "./starters";
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

const starter = (id: string) => target.querySelector<HTMLButtonElement>(`[data-starter=${id}]`);
const modeLabel = () => target.querySelector("[data-testid=agent-mode]")?.textContent?.trim();
const prompt = (id: string) => STARTERS.find((s) => s.id === id)?.prompt;

describe("AgentsPanel starters and summary", () => {
  it("starts a starter task in one press, in its own mode, and keeps the person's mode", async () => {
    const core = await setup();
    expect([...target.querySelectorAll("[data-starter] .label")].map((l) => l.textContent)).toEqual([
      "Explain this project",
      "Fix failing tests",
      "Write a README",
    ]);
    starter("explain")?.click();
    await settle();
    expect(core.sent("agents.start").map((c) => [c.params.mode, c.params.prompt])).toEqual([
      ["research", prompt("explain")],
    ]);
    // The list is no longer empty, so the starters are gone; the form keeps "One agent, here".
    [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === "Tasks")?.click();
    await settle();
    expect(target.querySelector("[data-testid=agent-starters]")).toBeNull();
    expect(modeLabel()).toBe("One agent, here");
  });

  it("explains as one agent here when research is off", async () => {
    const core = new FakeAgentCore();
    core.capacity = { working: 0, limit: 3, queued: 0, research_working: 0, research_limit: 0, waiting: [] };
    await setup(core);
    starter("explain")?.click();
    await settle();
    expect(core.sent("agents.start")[0]?.params.mode).toBe("direct");
  });

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

describe("AgentsPanel setup link", () => {
  const link = () =>
    [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === "Check setup");

  it("is a quiet link while the check finds nothing, or cannot run", async () => {
    await setup();
    expect(link()).toBeDefined();
    expect(target.querySelector("[data-testid=agents-setup-problems]")).toBeNull();
  });

  it("names how many problems the check found and opens Setup check", async () => {
    const core = new FakeAgentCore();
    core.setupReport = {
      agents: [{ id: "claude", name: "Claude", setup: { ...core.setup } }],
      git: { state: "checked", found: { installed: false, version: null } },
      gh: { state: "skipped" },
      sandbox: { state: "unavailable", reason: "install bubblewrap (bwrap) to sandbox agents" },
    };
    await setup(core);
    expect(core.sent("setup.check").map((c) => c.params)).toEqual([{ project: "p1" }]);
    expect(target.querySelector("[data-testid=agents-setup-problems]")?.textContent).toContain(
      "Setup check found 2 problems on this computer.",
    );
    link()?.click();
    flushSync();
    expect(settings.pane).toBe("setup");
    expect(mainView.current).toBe("settings");
    mainView.close(false);
  });
});
