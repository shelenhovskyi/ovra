import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
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

async function open(core: FakeAgentCore) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  const row = [...target.querySelectorAll<HTMLButtonElement>(".sessions .row-button")][0];
  row?.click();
  await settle();
}

const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === name);
const result = () => target.querySelector("[data-testid=go-back-result]")?.textContent?.replace(/\s+/g, " ").trim();

function twoTurns(): FakeAgentCore {
  const core = new FakeAgentCore();
  core.addSession("p1", "Write one", { state: "idle" });
  core.transcripts.set("s1", [
    { kind: "user_text", text: "Write one", from: "person" },
    { kind: "assistant_text", text: "Wrote one." },
    { kind: "user_text", text: "Write two", from: "person" },
  ]);
  core.checkpoints = [
    { turn: 2, at_ms: 2, message: "Write two", files_changed: 1 },
    { turn: 1, at_ms: 1, message: "Write one", files_changed: 1 },
  ];
  core.wentBack = { restored: ["two.txt"], conflicts: [] };
  return core;
}

it("goes back to a message: its text returns to the box, the result shows, and Undo puts the files back", async () => {
  const core = twoTurns();
  await open(core);
  const backs = [...target.querySelectorAll<HTMLButtonElement>("[data-testid=go-back]")];
  expect(backs).toHaveLength(2);
  backs[1]?.click();
  flushSync();
  button("Go back")?.click();
  await settle();
  expect(core.sent("agents.go_back").at(-1)?.params).toEqual({ session: "s1", turn: 2 });
  expect(target.querySelector<HTMLTextAreaElement>("#agent-reply")?.value).toBe("Write two");
  expect(target.querySelector("[data-testid=set-back-divider]")).not.toBeNull();
  expect(result()).toBe("Set back: two.txt. Undo go back");

  button("Undo go back")?.click();
  await settle();
  expect(core.sent("agents.undo_go_back")).toHaveLength(1);
  expect(result()).toBe("Put back: two.txt.");
  expect(target.querySelector("[data-testid=set-back-divider]")).toBeNull();
});

it("offers no going back in a task's coordinator", async () => {
  const core = twoTurns();
  core.tasks.push({
    id: "t1",
    project: "p1",
    coordinator: "s1",
    workers: [],
    limits: { max_workers: 3, budget_micro_usd: null },
    spent_micro_usd: 0,
  });
  await open(core);
  expect(target.querySelector("[data-testid=go-back]")).toBeNull();
  expect(core.sent("agents.checkpoints")).toHaveLength(0);
});
