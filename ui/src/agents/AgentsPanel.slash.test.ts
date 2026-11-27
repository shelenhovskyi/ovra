import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Workspace } from "../shell/workspace.svelte";
import { FakeAgentCore } from "./fake-core";
import { agentPrefs } from "./prefs.svelte";
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
  for (const t of agentPrefs.templates) agentPrefs.removeTemplate(t.id);
  agentPrefs.pickModel = false;
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

function field(): HTMLTextAreaElement {
  const box = target.querySelector<HTMLTextAreaElement>("#agent-prompt");
  if (!box) throw new Error("no field");
  return box;
}

async function type(text: string) {
  const box = field();
  box.focus();
  box.value = text;
  box.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
}

async function key(code: string) {
  field().dispatchEvent(new KeyboardEvent("keydown", { code, bubbles: true, cancelable: true }));
  await settle();
}

const rows = () =>
  [...target.querySelectorAll("[data-testid=slash-menu] [role=option]")].map((r) => r.textContent?.trim());

describe("AgentsPanel / menu and model", () => {
  it("offers templates and the project's commands after a leading /, and fills the box", async () => {
    agentPrefs.addTemplate("Review for bugs", "Review the uncommitted changes for bugs.");
    const core = new FakeAgentCore();
    core.catalog = [
      {
        kind: "command",
        name: "/ship",
        description: "Ships it",
        path: ".claude/commands/ship.md",
        agents: ["claude"],
        loads: "always",
        fields: [],
        files: [],
        problem: null,
      },
    ];
    await setup(core);
    await type("see src/x");
    expect(target.querySelector("[data-testid=slash-menu]")).toBeNull();
    await type("/");
    expect(rows()).toEqual(["Review for bugs Review the uncommitted changes for bugs.", "/ship Ships it"]);
    await key("Enter");
    expect(field().value).toBe("Review the uncommitted changes for bugs.");
    expect(core.sent("agents.start")).toEqual([]);

    await type("/sh");
    await key("Tab");
    expect(field().value).toBe("/ship ");
    // Escape leaves the text as typed, and / stays text until the box is empty.
    await type("/x");
    await key("Escape");
    expect(target.querySelector("[data-testid=slash-menu]")).toBeNull();
    await type("/xy");
    expect(target.querySelector("[data-testid=slash-menu]")).toBeNull();
  });

  it("points to Settings while there are no templates", async () => {
    await setup();
    await type("/");
    expect(rows()).toEqual(["Task templates… Save texts you use often, in Settings"]);
  });

  it("asks for the picked model only once Settings offers the picker, and remembers it", async () => {
    const core = await setup();
    expect(target.querySelector("[data-testid=model-picker]")).toBeNull();
    agentPrefs.pickModel = true;
    await settle();
    const picker = target.querySelector<HTMLSelectElement>("[data-testid=model-picker] select");
    if (!picker) throw new Error("no model picker");
    expect(picker.value).toBe("");
    picker.value = "haiku";
    picker.dispatchEvent(new Event("change", { bubbles: true }));
    await type("What does this do?");
    target.querySelector<HTMLFormElement>("form")?.requestSubmit();
    await settle();
    expect(core.sent("agents.start").map((c) => c.params.model)).toEqual(["haiku"]);
    expect(localStorage.getItem("ovra.task-model.p1")).toBe("haiku");
  });
});
