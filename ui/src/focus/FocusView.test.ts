import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { agentSessions } from "../agents/sessions.svelte";
import { review } from "../inbox/fake-review";
import { registerCommand } from "../keys/commands";
import { Workspace } from "../shell/workspace.svelte";
import { focusMode } from "./mode.svelte";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  focusMode.set(true);
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
  focusMode.set(false);
});

async function setup(prepare: (core: FakeAgentCore) => void) {
  const core = new FakeAgentCore();
  prepare(core);
  const workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  await agentSessions(core).load("p1");
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  flushSync();
  await Promise.resolve();
  flushSync();
  return core;
}

const text = (id: string) => target.querySelector(`[data-testid=${id}]`)?.textContent?.replace(/\s+/g, " ").trim();

describe("FocusView", () => {
  it("shows the project, what runs, what is next and what is done", async () => {
    await setup((core) => {
      core.addSession("p1", "Fix sync", { state: "working" });
      core.addSession("p1", "Update docs", { state: "queued" });
      core.addSession("p1", "Add a test", { state: "idle" });
    });
    expect(text("focus-head")).toBe("demo 1 working · 1 next · 1 done");
    expect(text("focus-progress")).toBe("In progress1 Fix syncWorking");
    expect(text("focus-next")).toBe("Next1 Update docsWaiting for a free agent");
    expect(text("focus-done")).toBe("Done1 Add a testDone");
    expect(text("focus-nothing")).toBe("Nothing waits for you.");
  });

  it("loads the plan of an agent at work and shows its next steps", async () => {
    await setup((core) => {
      core.addSession("p1", "Fix sync", { state: "working" });
      core.transcripts.set("s1", [
        {
          kind: "tool_use",
          id: "t1",
          name: "TodoWrite",
          input: { todos: [{ content: "Write a <i>test</i>", status: "pending" }] },
        },
      ]);
    });
    await vi.waitFor(() => expect(text("focus-next")).toBe("Next1 Write a <i>test</i>Planned by Fix sync"));
    expect(target.querySelector("[data-testid=focus-next] i")).toBeNull();
  });

  it("shows an agent's question from its task's board and sends the answer as text", async () => {
    const core = await setup((core) => {
      const coordinator = core.addSession("p1", "Make sync robust", { state: "idle" });
      core.tasks = [
        {
          id: "t1",
          project: "p1",
          coordinator: coordinator.id,
          workers: [],
          limits: { max_workers: 3, budget_micro_usd: null },
          spent_micro_usd: 0,
        },
      ];
      core.boards.set("t1", [
        {
          id: "b1",
          author: { kind: "worker", name: "retry" },
          body: { kind: "question", to: { kind: "person" }, text: "Give up after 5 tries?" },
        },
        {
          id: "b2",
          author: { kind: "worker", name: "docs" },
          body: { kind: "question", to: { kind: "coordinator" }, text: "Not for you" },
        },
      ]);
    });
    await vi.waitFor(() => expect(target.querySelectorAll("[data-testid=focus-question]")).toHaveLength(1));
    const card = target.querySelector("[data-testid=focus-question]");
    expect(card?.querySelector(".who")?.textContent).toBe("Question from retry · Make sync robust");
    expect(card?.querySelector("h3")?.textContent).toBe("Give up after 5 tries?");
    expect(target.querySelector("[data-testid=focus-nothing]")).toBeNull();
    const box = card?.querySelector("textarea") as HTMLTextAreaElement;
    box.value = "Yes, after 5";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    card?.querySelector<HTMLButtonElement>("button[type=submit]")?.click();
    await vi.waitFor(() => expect(target.querySelectorAll("[data-testid=focus-question]")).toHaveLength(0));
    expect(core.calls.find((c) => c.method === "board.answer")?.params).toEqual({
      task: "t1",
      question: "b1",
      text: "Yes, after 5",
    });
  });

  it("shows a review that waits, and See changes leaves Focus for Reviews", async () => {
    const opened = vi.fn();
    const off = registerCommand({ id: "inbox.toggle", title: "Reviews", run: opened });
    await setup((core) => {
      core.addSession("p1", "Offline banner", { state: "idle" });
      core.reviews = [review("r1", { session: "s1", summary: "Shows <b>Offline</b> in the header." })];
    });
    const card = target.querySelector("[data-testid=focus-review]");
    expect(card?.querySelector("h3")?.textContent).toBe("Offline banner");
    // Agent text is text, never markup.
    expect(card?.querySelector(".summary")?.textContent).toBe("Shows <b>Offline</b> in the header.");
    expect(card?.querySelector("b")).toBeNull();
    expect(card?.textContent).toContain("1 file changed");
    expect(target.querySelector("[data-testid=focus-nothing]")).toBeNull();
    card?.querySelector("button")?.click();
    expect(focusMode.on).toBe(false);
    expect(opened).toHaveBeenCalled();
    off();
  });

  it("leaves Focus for the IDE and for a new task", async () => {
    const started = vi.fn();
    const off = registerCommand({ id: "agents.new_session", title: "New task", run: started });
    await setup(() => {});
    expect(text("focus-head")).toBe("demo No agents yet");
    target.querySelector<HTMLButtonElement>("[data-testid=focus-open-ide]")?.click();
    expect(focusMode.on).toBe(false);
    focusMode.set(true);
    target.querySelector<HTMLButtonElement>("[data-testid=focus-new-task]")?.click();
    expect(focusMode.on).toBe(false);
    await vi.waitFor(() => expect(started).toHaveBeenCalled());
    off();
  });
});
