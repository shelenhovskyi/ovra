import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
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
  agentPrefs.planFirst = false;
  localStorage.clear();
});

async function settle() {
  flushSync();
  await new Promise((resolve) => setImmediate(resolve));
  flushSync();
}

async function open(core: FakeAgentCore, row = true) {
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  if (row) target.querySelector<HTMLButtonElement>(".sessions .row-button")?.click();
  await settle();
}

const button = (name: string) =>
  [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === name);
const field = (id: string) => target.querySelector<HTMLTextAreaElement>(`#${id}`);

function type(id: string, text: string) {
  const box = field(id);
  if (!box) throw new Error(`no #${id}`);
  box.value = text;
  box.dispatchEvent(new Event("input"));
  flushSync();
}

it("queues a reply while the agent works, then edits, removes and sends it now", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Fix it", { state: "working" });
  await open(core);
  type("agent-reply", "also the tests");
  expect(button("Queue")?.disabled).toBe(false);
  button("Queue")?.click();
  await settle();
  expect(core.sent("agents.queue").map((c) => c.params)).toEqual([{ session: "s1", text: "also the tests" }]);
  expect(field("agent-reply")?.value).toBe("");
  const rows = () =>
    [...target.querySelectorAll("[data-testid=queued-message]")].map((r) => r.querySelector(".text")?.textContent);
  expect(rows()).toEqual(["also the tests"]);

  // Edit takes it back into the box.
  target.querySelector<HTMLButtonElement>('[aria-label="Edit \\"also the tests\\""]')?.click();
  await settle();
  expect(core.sent("agents.unqueue").map((c) => c.params)).toEqual([{ session: "s1", id: 1 }]);
  expect(field("agent-reply")?.value).toBe("also the tests");
  expect(rows()).toEqual([]);

  button("Queue")?.click();
  await settle();
  button("Send now")?.click();
  await settle();
  expect(core.sent("agents.interrupt")).toHaveLength(1);
  target.querySelector<HTMLButtonElement>('[aria-label="Remove \\"also the tests\\""]')?.click();
  await settle();
  expect(rows()).toEqual([]);

  // Idle again: the reply goes at once.
  core.status("s1", { state: "idle" });
  await settle();
  type("agent-reply", "thanks");
  button("Send")?.click();
  await settle();
  expect(core.sent("agents.send").map((c) => c.params.text)).toEqual(["thanks"]);
});

it("offers Plan first only once turned on, and approves the plan when it is ready", async () => {
  const core = new FakeAgentCore();
  await open(core, false);
  expect(target.querySelector("[data-testid=plan-first]")).toBeNull();
  unmount(app ?? {});
  app = null;
  agentPrefs.planFirst = true;
  await open(core, false);
  const box = target.querySelector<HTMLInputElement>("[data-testid=plan-first] input");
  expect(box?.checked).toBe(true);
  type("agent-prompt", "Fix the login");
  button("Start")?.click();
  await settle();
  const start = core.calls.find((c) => c.method === "agents.start" || c.method === "orchestration.start");
  expect(start?.params).toMatchObject({ plan_first: true });

  const bar = () => target.querySelector("[data-testid=plan-bar]")?.textContent?.replace(/\s+/g, " ").trim();
  // Not started yet: no plan to approve.
  expect(bar()).toBe("Planning. The agent changes no files until you approve its plan.");
  expect(button("Approve plan")).toBeUndefined();
  core.status("s1", { state: "working" });
  await settle();
  expect(bar()).toBe("Planning. The agent changes no files until you approve its plan.");
  core.status("s1", { state: "idle" });
  await settle();
  button("Approve plan")?.click();
  await settle();
  expect(core.sent("agents.approve_plan").map((c) => c.params)).toEqual([{ session: "s1" }]);
  expect(bar()).toBeUndefined();
});
