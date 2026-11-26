import { describe, expect, it } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { AgentSessions } from "../agents/sessions.svelte";
import { review } from "../inbox/fake-review";
import type { AgentMessage } from "../lib/protocol";
import { countLine, focusSections, pendingSteps } from "./model";

const todo = (input: unknown, id = "t1"): AgentMessage => ({ kind: "tool_use", id, name: "TodoWrite", input });

async function loaded(core: FakeAgentCore): Promise<AgentSessions> {
  const sessions = new AgentSessions(core);
  await sessions.load("p1");
  return sessions;
}

describe("focusSections", () => {
  it("sorts sessions into in progress, next and done, with their words", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix sync", { state: "working" });
    core.addSession("p1", "Retry backoff", { state: "waiting_for_approval" });
    core.addSession("p1", "Update docs", { state: "queued" });
    core.addSession("p1", "Add a test", { state: "idle" });
    core.addSession("p1", "Old try", { state: "failed", message: "usage limit reached" });
    core.addSession("p2", "Elsewhere", { state: "working" });
    const sections = focusSections(await loaded(core), "p1");
    expect(sections.progress).toEqual([
      { id: "s1", name: "Fix sync", status: "Working" },
      { id: "s2", name: "Retry backoff", status: "Needs approval" },
    ]);
    expect(sections.next.map((r) => r.name)).toEqual(["Update docs"]);
    expect(sections.done.map((r) => [r.name, r.status])).toEqual([
      ["Add a test", "Done"],
      ["Old try", "Failed: usage limit reached"],
    ]);
    expect(countLine(sections)).toBe("2 working · 1 next · 2 done");
  });

  it("lists sessions whose changes wait for review and leaves out closed ones", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Offline banner", { state: "idle" });
    core.addSession("p1", "Closed one", { state: "idle" });
    core.reviews = [review("r1", { session: "s1" })];
    const sessions = await loaded(core);
    sessions.closed.add("s2");
    const sections = focusSections(sessions, "p1");
    expect(sections.reviews).toEqual(["s1"]);
    // Finished but not kept yet: it waits for the person, so it is not "done".
    expect(sections.done.map((r) => [r.name, r.status])).toEqual([["Offline banner", "Waiting for review"]]);
    expect(countLine(sections)).toBe("1 to review");
  });

  it("keeps a task in progress while its workers run", async () => {
    const core = new FakeAgentCore();
    const coordinator = core.addSession("p1", "Make sync robust", { state: "idle" });
    const worker = core.addSession("p1", "Retry", { state: "working" });
    core.tasks = [
      {
        id: "t1",
        project: "p1",
        coordinator: coordinator.id,
        workers: [{ session: worker.id, name: "retry", branch: "ovra/t1/retry", worktree: "/w", kept: null }],
        limits: { max_workers: 3, budget_micro_usd: null },
        spent_micro_usd: 0,
      },
    ];
    const sessions = await loaded(core);
    const sections = focusSections(sessions, "p1");
    expect(sections.progress.map((r) => r.status)).toEqual(["Workers are on it", "Working"]);
    // Once the worker is done, the coordinator, which only made workers, is not counted.
    sessions.byId.set(worker.id, { ...worker, status: { state: "idle" } });
    expect(countLine(focusSections(sessions, "p1"))).toBe("1 done");
  });

  it("adds the pending steps of an agent at work to Next, after the agents that wait", async () => {
    const core = new FakeAgentCore();
    core.addSession("p1", "Fix sync", { state: "working" });
    core.addSession("p1", "Update docs", { state: "queued" });
    core.addSession("p1", "Old one", { state: "idle" });
    const sessions = await loaded(core);
    const plan = {
      todos: [
        { content: "Read the sync code", status: "completed" },
        { content: "Add retries", status: "in_progress" },
        { content: "Write a test\nfor retries", status: "pending" },
      ],
    };
    sessions.transcripts.set("s1", [todo(plan)]);
    // A finished agent's leftover plan is not "next".
    sessions.transcripts.set("s3", [todo({ todos: [{ content: "Never mind", status: "pending" }] })]);
    const sections = focusSections(sessions, "p1");
    expect(sections.next.map((r) => [r.name, r.status])).toEqual([
      ["Update docs", "Waiting for a free agent"],
      ["Write a test for retries", "Planned by Fix sync"],
    ]);
  });

  it("reads only the latest to-do list, and nothing from one of another shape", () => {
    const first = todo({ todos: [{ content: "Old step", status: "pending" }] });
    const latest = todo({ todos: [{ content: "New step", status: "pending" }] }, "t2");
    expect(pendingSteps([first, latest])).toEqual(["New step"]);
    expect(pendingSteps([first, todo({ todos: "all of it" }, "t3")])).toEqual([]);
    expect(pendingSteps([todo(null)])).toEqual([]);
    expect(pendingSteps([todo({ todos: [{ content: 7, status: "pending" }, { status: "pending" }] })])).toEqual([]);
    const long = "x".repeat(400);
    expect(pendingSteps([todo({ todos: [{ content: long, status: "pending" }] })])[0]).toHaveLength(160);
    const many = Array.from({ length: 20 }, (_, i) => ({ content: `Step ${i}`, status: "pending" }));
    expect(pendingSteps([todo({ todos: many })])).toHaveLength(8);
  });

  it("says when there is nothing yet", () => {
    expect(countLine({ progress: [], next: [], done: [], reviews: [], finished: 0 })).toBe("No agents yet");
  });
});
