import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
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
  localStorage.clear();
});

async function settle() {
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

async function setup(core = new FakeAgentCore()) {
  workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

function type(field: HTMLTextAreaElement | null, text: string) {
  if (!field) throw new Error("no field");
  field.focus();
  field.value = text;
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Picks how the next task runs from the mode menu, as clicks would. */
function choose(label: string) {
  target.querySelector<HTMLButtonElement>("[data-testid=agent-mode]")?.click();
  flushSync();
  const option = [...target.querySelectorAll<HTMLButtonElement>("[role=menuitemradio]")].find(
    (b) => b.querySelector(".name")?.textContent === label,
  );
  option?.click();
  flushSync();
}

const modeLabel = () => target.querySelector("[data-testid=agent-mode]")?.textContent?.trim();

const startButton = () =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Start");

const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === name);

describe("AgentsPanel research", () => {
  it("starts research apart from the other agents and saves or hands on its report", async () => {
    const core = new FakeAgentCore();
    core.capacity = { working: 3, limit: 3, queued: 0, research_working: 1, research_limit: 2, waiting: [] };
    await setup(core);
    choose("Research");
    await settle();
    expect(modeLabel()).toBe("Research");
    expect(target.querySelector("#agent-mode-hint")?.textContent).toBe(
      "Reads this folder and searches the web. Changes nothing.",
    );
    // Every worker place is taken, but research has its own.
    expect(target.querySelector("[data-testid=agents-capacity]")?.textContent?.trim()).toBe("3 of 3 · 1 researching");
    type(target.querySelector("#agent-prompt"), "How does review work?");
    await settle();
    expect(startButton()?.disabled).toBe(false);
    startButton()?.click();
    await settle();
    expect(core.sent("agents.start")[0]?.params.mode).toBe("research");

    core.message("s1", { kind: "assistant_text", text: "## Answer\nThe inbox." });
    core.status("s1", { state: "idle" });
    await settle();
    button("Save as doc")?.click();
    await settle();
    const day = new Date().toISOString().slice(0, 10);
    const path = `docs/research/${day}-how-does-review-work.md`;
    expect(core.files.get(path)).toBe("# How does review work?\n\n## Answer\nThe inbox.\n");
    expect(target.querySelector("[data-testid=agent-report]")?.textContent).toContain(`Saved as ${path}`);
    expect(workspace.openPath).toBe(path);

    button("Turn into task")?.click();
    await settle();
    expect(modeLabel()).toBe("Plan and split");
    const field = target.querySelector<HTMLTextAreaElement>("#agent-prompt");
    expect(field?.value).toBe('Research on "How does review work?" found:\n\n> ## Answer\n> The inbox.\n\nTask: ');
    expect(document.activeElement).toBe(field);
    // Nothing starts until the person says what to do.
    expect(core.sent("orchestration.start")).toEqual([]);
  });

  it("numbers a saved report whose name is taken", async () => {
    const core = new FakeAgentCore();
    const day = new Date().toISOString().slice(0, 10);
    core.files.set(`docs/research/${day}-why.md`, "old");
    await setup(core);
    choose("Research");
    type(target.querySelector("#agent-prompt"), "Why");
    await settle();
    startButton()?.click();
    await settle();
    core.message("s1", { kind: "assistant_text", text: "Because." });
    core.status("s1", { state: "idle" });
    await settle();
    button("Save as doc")?.click();
    await settle();
    expect(core.files.get(`docs/research/${day}-why.md`)).toBe("old");
    expect(core.files.get(`docs/research/${day}-why-2.md`)).toBe("# Why\n\nBecause.\n");
  });

  it("says when research is off or full instead of starting it", async () => {
    const core = new FakeAgentCore();
    core.capacity = { working: 0, limit: 3, queued: 0, research_working: 0, research_limit: 0, waiting: [] };
    await setup(core);
    choose("Research");
    type(target.querySelector("#agent-prompt"), "Look");
    await settle();
    expect(startButton()?.title).toBe("Research is off in Settings");
    core.capacity = { working: 0, limit: 3, queued: 0, research_working: 2, research_limit: 2, waiting: [] };
    core.emit({ type: "agents.research_limit_changed", data: { limit: 2, default: 2, min: 0, max: 5 } });
    await settle();
    expect(startButton()?.title).toBe("All 2 research agents are busy");
    expect(startButton()?.disabled).toBe(true);
  });
});
