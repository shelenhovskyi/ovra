import { describe, expect, it } from "vitest";
import { FakeAgentCore } from "../agents/fake-core";
import type { BoardEntry } from "../lib/protocol";
import { TaskBoards } from "./boards.svelte";

const note = (id: string, text: string): BoardEntry => ({
  id,
  author: { kind: "coordinator" },
  body: { kind: "note", text },
});

describe("task boards", () => {
  it("keeps an entry that came while the board loaded, in board order, once", async () => {
    const core = new FakeAgentCore();
    core.boards.set("t1", [note("b1", "one")]);
    const boards = new TaskBoards(core);
    boards.watch("t1");
    core.emit({ type: "board.changed", data: { project: "p1", task: "t1", entry: note("b2", "two") } });
    await new Promise((resolve) => setImmediate(resolve));
    core.emit({ type: "board.changed", data: { project: "p1", task: "t1", entry: note("b2", "two") } });
    expect(boards.entries.get("t1")?.map((e) => e.id)).toEqual(["b1", "b2"]);
  });

  it("counts only questions for the person that nobody answered", async () => {
    const core = new FakeAgentCore();
    const lint = { kind: "worker", name: "lint" } as const;
    core.boards.set("t1", [
      { id: "b1", author: lint, body: { kind: "question", to: { kind: "person" }, text: "A?" } },
      { id: "b2", author: lint, body: { kind: "question", to: { kind: "coordinator" }, text: "B?" } },
      { id: "b3", author: lint, body: { kind: "question", to: { kind: "person" }, text: "C?" } },
      { id: "b4", author: { kind: "person" }, body: { kind: "answer", question: "b1", text: "yes" } },
    ]);
    const boards = new TaskBoards(core);
    boards.watch("t1");
    await new Promise((resolve) => setImmediate(resolve));
    expect(boards.openForPerson("t1").map((e) => e.id)).toEqual(["b3"]);
  });
});
