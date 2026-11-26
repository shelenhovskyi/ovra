import { expect, it } from "vitest";
import { TaskBoards } from "../board/boards.svelte";
import { change, review } from "../inbox/fake-review";
import type { Task } from "../lib/protocol";
import { FakeAgentCore } from "./fake-core";
import { TaskRows } from "./rows";
import { AgentSessions } from "./sessions.svelte";

const end = { kind: "turn_end" as const, usage: { input_tokens: 1, output_tokens: 1, cost_micro_usd: null } };

function task(workers: [session: string, name: string][], extra: Partial<Task> = {}): Task {
  return {
    id: "t1",
    project: "p1",
    coordinator: "s1",
    workers: workers.map(([session, name]) => ({
      session,
      name,
      branch: `ovra/t1/${name}`,
      worktree: `/wt/${name}`,
      kept: null,
    })),
    limits: { max_workers: 3, budget_micro_usd: null },
    spent_micro_usd: 0,
    ...extra,
  };
}

/** The rows of project p1 as the panel lists them: title, state, call to action and progress. */
async function rowsOf(core: FakeAgentCore) {
  const sessions = new AgentSessions(core);
  const boards = new TaskBoards(core);
  await sessions.load("p1");
  for (const s of sessions.inProject("p1")) await sessions.loadTranscript(s.id);
  for (const t of core.tasks) boards.watch(t.id);
  await new Promise((resolve) => setImmediate(resolve));
  const rows = new TaskRows(sessions, boards);
  const list = sessions.shown("p1");
  return {
    rows,
    shown: () =>
      rows.ordered(list).map((s) => {
        const parts = rows.progress(s.id);
        const chip = rows.chip(s, list)?.text;
        return [rows.title(s), sessions.status(s.id), rows.tone(s), parts && `${parts.done}/${parts.total}`, chip]
          .filter(Boolean)
          .join(" | ");
      }),
    list,
  };
}

it("lists a task's workers under it in the order it started them, by their own names", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Write the notes", { state: "working" });
  // Known in another order than they were started (lint's status came first).
  for (const name of ["lint", "docs"]) core.addSession("p1", `${name} notes`, { state: "working" });
  core.addSession("p1", "A plain session", { state: "idle" });
  core.tasks.push(task([["s3", "docs"]]));
  core.tasks[0]?.workers.push({
    session: "s2",
    name: "lint",
    branch: "ovra/t1/lint",
    worktree: "/wt/lint",
    kept: "/kept/lint",
  });
  const { rows, shown, list } = await rowsOf(core);
  expect(shown()).toEqual([
    "Write the notes | Working | working | 0/2",
    "docs · docs notes | Working | working",
    "lint · lint notes | Working | working",
    "A plain session | Done | idle",
  ]);
  // The whole name shows on hover, should it be cut, with a worker's branch and kept folder.
  expect(rows.ordered(list).map((s) => rows.tip(s))).toEqual([
    "Task: Write the notes",
    "docs: docs notes\nBranch ovra/t1/docs",
    "lint: lint notes\nBranch ovra/t1/lint\nIts folder was kept: /kept/lint",
    "A plain session",
  ]);
});

it("puts what needs the person first, then work in progress, then queued, keeping a task's workers under it", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Queued", { state: "queued" });
  core.addSession("p1", "Ended", { state: "stopped" });
  core.addSession("p1", "Working", { state: "working" });
  core.addSession("p1", "Asks", { state: "waiting_for_approval" });
  const { shown } = await rowsOf(core);
  expect(shown().map((r) => r.split(" | ")[0])).toEqual(["Asks", "Working", "Queued", "Ended"]);
  expect(shown()[0]).toBe("Asks | Needs approval | waiting_for_approval | Approve");
});

it("keeps a task Done when it is stopped after all its workers finished", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Write the notes", { state: "stopped" });
  core.addSession("p1", "docs notes", { state: "stopped" });
  core.addSession("p1", "Still working", { state: "working" });
  for (const id of ["s1", "s2"]) core.transcripts.set(id, [end]);
  core.tasks.push(task([["s2", "docs"]]));
  const { shown } = await rowsOf(core);
  expect(shown()).toEqual([
    "Still working | Working | working",
    "Write the notes | Done | idle | 1/1",
    "docs · docs notes | Done | idle",
  ]);
});

it("never calls a task done when its workers failed or did not start", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Write the notes", { state: "idle" });
  core.addSession("p1", "docs notes", { state: "failed", message: "claude was not found" });
  core.tasks.push(task([["s2", "docs"]], { failed: [{ name: "lint", reason: "the worktree could not be made" }] }));
  const { shown } = await rowsOf(core);
  expect(shown()).toEqual([
    "Write the notes | Failed: 2 of 2 workers | failed | 0/2",
    "docs · docs notes | Failed: claude was not found | failed",
  ]);
});

it("says on the task how many workers' changes wait, and never again on its workers", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Write the notes", { state: "idle" });
  for (const name of ["docs", "lint"]) {
    const worker = core.addSession("p1", `${name} notes`, { state: "stopped" });
    core.transcripts.set(worker.id, [end]);
  }
  core.tasks.push(
    task([
      ["s2", "docs"],
      ["s3", "lint"],
    ]),
  );
  // lint's newest item is decided, an older one still waits: it counts.
  core.reviews = [
    review("new", { session: "s3", state: "approved" }),
    review("r3", { session: "s3", files: [change("NOTES.md")] }),
    review("r2", { session: "s2", files: [change("NOTES.md")] }),
  ];
  const { shown } = await rowsOf(core);
  expect(shown()).toEqual([
    "Write the notes | Done | idle | 2/2 | Review 2",
    "docs · docs notes | Done | idle",
    "lint · lint notes | Done | idle",
  ]);
});

it("asks for an answer on a task whose board has an open question for the person", async () => {
  const core = new FakeAgentCore();
  core.addSession("p1", "Plain", { state: "working" });
  core.addSession("p1", "Write the notes", { state: "idle" });
  core.addSession("p1", "docs notes", { state: "working" });
  core.tasks.push(task([["s3", "docs"]], { coordinator: "s2" }));
  core.boards.set("t1", [
    {
      id: "b1",
      author: { kind: "worker", name: "docs" },
      body: { kind: "question", to: { kind: "person" }, text: "?" },
    },
  ]);
  const { shown } = await rowsOf(core);
  expect(shown().slice(0, 2)).toEqual([
    "Write the notes | Workers are on it | idle | 0/1 | Answer",
    "docs · docs notes | Working | working",
  ]);
});
