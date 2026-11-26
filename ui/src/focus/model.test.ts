import { describe, expect, it } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import { AgentSessions } from "../agents/sessions.svelte";
import { review } from "../inbox/fake-review";
import { countLine, focusSections } from "./model";

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
    expect(sections.done.map((r) => r.name)).toEqual(["Offline banner"]);
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
    const sections = focusSections(await loaded(core), "p1");
    expect(sections.progress.map((r) => r.status)).toEqual(["Workers are on it", "Working"]);
  });

  it("says when there is nothing yet", () => {
    expect(countLine({ progress: [], next: [], done: [], reviews: [] })).toBe("No agents yet");
  });
});
