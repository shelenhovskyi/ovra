import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
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

const ANSWER = "Found:\n\n1. Race in watch.rs\n2. Slow test\n3. Typo in README\n\nShall I fix them?";

async function open(core: FakeAgentCore) {
  core.addSession("p1", "Find bugs", { state: "idle" });
  core.transcripts.set("s1", [
    { kind: "user_text", text: "Find bugs", from: "person" },
    { kind: "assistant_text", text: ANSWER },
  ]);
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  [...target.querySelectorAll<HTMLButtonElement>(".row-button")]
    .find((b) => b.textContent?.includes("Find bugs"))
    ?.click();
  await settle();
}

const card = () => target.querySelector("[data-testid=agent-items]");
const button = () => target.querySelector<HTMLButtonElement>("[data-testid=agent-items-start]");
const line = () => target.querySelector("[data-testid=agent-items-line]")?.textContent;
function pick(...ns: number[]) {
  const boxes = [...target.querySelectorAll<HTMLInputElement>("[data-testid=agent-items] input")];
  for (const n of ns) boxes[n - 1]?.click();
  flushSync();
}

describe("AgentsPanel numbered answer to tasks", () => {
  it("starts one agent in its own copy for each picked item", async () => {
    const core = new FakeAgentCore();
    core.capacity = { ...core.capacity, working: 1, limit: 3 };
    await open(core);
    expect([...(card()?.querySelectorAll(".title") ?? [])].map((t) => t.textContent)).toEqual([
      "Race in watch.rs",
      "Slow test",
      "Typo in README",
    ]);
    expect(button()?.disabled).toBe(true);
    pick(3, 1);
    expect(button()?.textContent).toBe("Do 1 and 3");
    pick(2);
    expect(line()).toBe("2 start now, 1 waits for a free agent.");
    button()?.click();
    await settle();
    expect(core.sent("orchestration.start").map((c) => [c.params.solo, c.params.prompt.split("\n")[0]])).toEqual([
      [true, 'Item 1 of the answer to "Find bugs":'],
      [true, 'Item 2 of the answer to "Find bugs":'],
      [true, 'Item 3 of the answer to "Find bugs":'],
    ]);
    expect(line()).toContain("Started items 1, 2 and 3");
    expect(button()).toBeNull();
  });

  it("says why when the project has no git, and shows nothing while the agent works", async () => {
    const core = new FakeAgentCore();
    core.noRepo = true;
    await open(core);
    pick(1);
    expect(button()?.disabled).toBe(true);
    expect(line()).toBe("Needs git: each item works on its own copy of the project.");
    core.status("s1", { state: "working" });
    await settle();
    expect(card()).toBeNull();
  });
});
