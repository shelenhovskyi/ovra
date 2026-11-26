import { describe, expect, it } from "vitest";
import type { BoardEntry } from "../lib/protocol";
import { authorName, entryHead, entryText, held, summary } from "./describe";

const lint = { kind: "worker", name: "lint" } as const;
const entries: BoardEntry[] = [
  { id: "b1", author: lint, body: { kind: "claim", worker: "lint", paths: ["src/a.ts", "src/b.ts"], note: "rename" } },
  { id: "b2", author: { kind: "coordinator" }, body: { kind: "claim", worker: "docs", paths: ["docs/"], note: "" } },
  { id: "b3", author: lint, body: { kind: "question", to: { kind: "person" }, text: "Keep the old flag?" } },
  { id: "b4", author: { kind: "person" }, body: { kind: "answer", question: "b3", text: "No." } },
  { id: "b5", author: lint, body: { kind: "release", worker: "lint", paths: ["src/a.ts"] } },
];

describe("board entries in words", () => {
  it("names who wrote it and what it is", () => {
    expect(entries.map((e) => `${authorName(e.author)}: ${entryHead(e, entries)}: ${entryText(e)}`)).toEqual([
      "lint: Takes: src/a.ts, src/b.ts · rename",
      "Coordinator: Gives docs: docs/",
      "lint: Asks you: Keep the old flag?",
      "You: Answers lint: No.",
      "lint: Gives back: src/a.ts",
    ]);
  });

  it("keeps what each worker holds after it gives some back", () => {
    expect([...held(entries)]).toEqual([
      ["lint", ["src/b.ts"]],
      ["docs", ["docs/"]],
    ]);
  });

  it("sums the board up in one line", () => {
    expect(summary([], 0)).toBe("Nothing on the board yet.");
    expect(summary(entries, 1)).toBe("lint holds 1 path · docs holds 1 path · 1 question for you");
    expect(summary(entries.slice(2, 4), 0)).toBe("2 entries");
  });
});
