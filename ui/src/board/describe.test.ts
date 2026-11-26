import { describe, expect, it } from "vitest";
import type { BoardEntry } from "../lib/protocol";
import { authorName, covers, entryHead, entryText, entryTime, held, summary, whoChanges, whoText } from "./describe";

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

  it("says when an entry was written, with the day once it is not today", () => {
    const now = new Date(2026, 10, 26, 18, 0);
    const note: BoardEntry = { id: "b9", author: lint, body: { kind: "note", text: "done" } };
    const at = (d: Date): BoardEntry => ({ ...note, at_ms: d.getTime() });
    expect(entryTime(at(new Date(2026, 10, 26, 9, 5)), now)).toBe("09:05");
    expect(entryTime(at(new Date(2026, 10, 25, 23, 59)), now)).toBe("Nov 25, 23:59");
    expect(entryTime(note, now)).toBe("");
  });

  it("matches claims as the core does", () => {
    expect(covers("src/*.ts", "src/a.ts")).toBe(true);
    expect(covers("src/*.ts", "src/x/a.ts")).toBe(false);
    expect(covers("src/**", "src/x/a.ts")).toBe(true);
    expect(covers("docs/", "docs")).toBe(true);
    expect(covers("docs/", "docs/a.md")).toBe(true);
    expect(covers("docs/", "docsx/a.md")).toBe(false);
    expect(covers("a+b(1).md", "a+b(1).md")).toBe(true);
    expect(covers("a.md", "aXmd")).toBe(false);
  });

  it("lists who changes what, where two workers meet first", () => {
    const rows = whoChanges(entries, [
      { worker: "docs", paths: ["src/b.ts", "docs/guide.md"] },
      { worker: "lint", paths: ["src/c.ts"] },
    ]);
    expect(rows.map((r) => `${r.path}: ${whoText(r)}${r.overlap ? " (overlap)" : ""}`)).toEqual([
      "src/b.ts: lint claims it · docs changed it (overlap)",
      "docs/guide.md: docs claims it · docs changed it",
      "src/c.ts: lint changed it",
    ]);
    // A claim no change falls under shows as itself.
    expect(whoChanges(entries, []).map((r) => r.path)).toEqual(["docs/", "src/b.ts"]);
    expect(summary(entries, 0, 1)).toBe("lint holds 1 path · docs holds 1 path · 1 file shared between workers");
  });
});
