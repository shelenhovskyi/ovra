import { describe, expect, it } from "vitest";
import {
  approveHint,
  defaultCommitMessage,
  fileMarkers,
  fileSummary,
  itemSummary,
  notCopiedHint,
  protectedHint,
  rejectHint,
  sharedFiles,
} from "./describe";
import { change, review } from "./fake-review";

describe("inbox wording", () => {
  it("marks files changed outside the agent and files waiting for the direct agent", () => {
    expect(fileMarkers(change("a"))).toEqual([]);
    expect(fileMarkers(change("a", { by: "outside" }))).toEqual(["Changed outside the agent"]);
    expect(fileMarkers(change("a", { by: "both", blocked: true }))).toEqual([
      "Also changed outside the agent",
      "Waits for the direct agent",
    ]);
    expect(fileSummary(change("b", { kind: "renamed", from: "a", additions: 0, deletions: 0 }))).toBe(
      "renamed from a, +0 −0",
    );
  });

  it("marks protected files first and lists what a worker's approve would copy", () => {
    const item = review("r1", {
      mode: "isolated",
      files: [
        change("src/a.ts"),
        change(".claude/settings.json", { protected: true }),
        change("docs/CLAUDE.md", { protected: true, kind: "renamed", from: "CLAUDE.md" }),
        change(".mcp.json", { protected: true, blocked: true }),
      ],
    });
    expect(fileMarkers(change("a", { protected: true, by: "outside" }))).toEqual([
      "Protected: agent or tool configuration",
      "Changed outside the agent",
    ]);
    expect(itemSummary(item)).toBe("4 files, 3 protected, 1 waiting for the direct agent");
    expect(protectedHint(item)).toBe(
      "Changes agent configuration or code that tools run on their own: .claude/settings.json, docs/CLAUDE.md (from CLAUDE.md), .mcp.json (waits for the direct agent). Approve copies these into your project folder too; check their diffs first.",
    );
    expect(protectedHint(review("r2"))).toBeNull();
    expect(itemSummary(review("r3", { files: [change(".envrc", { by: "outside", protected: true })] }))).toBe(
      "1 file, 1 protected, all changed outside the agent",
    );
  });

  it("says a direct session's protected files are already in the project folder", () => {
    const item = review("r1", {
      mode: "direct",
      files: [change("CLAUDE.md", { protected: true }), change("src/a.ts")],
    });
    expect(protectedHint(item)).toBe(
      "Changes agent configuration or code that tools run on their own, already in your project folder: CLAUDE.md. Approve keeps them; Reject restores the agent's changes to them.",
    );
  });

  it("sums up an item and says what approve and reject do", () => {
    const worker = review("r1", {
      mode: "isolated",
      files: [change("a"), change("b", { blocked: true }), change("c", { by: "outside" })],
      summary: "Fix the typo\n\nDetails",
    });
    expect(itemSummary(worker)).toBe("3 files, 1 changed outside the agent, 1 waiting for the direct agent");
    expect(approveHint(worker)).toContain("except files that wait for the direct agent");
    expect(approveHint(worker)).toContain("A file you changed yourself since the worker started is not copied");
    expect(approveHint(review("r4"))).toBe("Approve keeps the changes in your project folder.");
    expect(rejectHint(worker)).toBe(
      "Reject discards the worker's changes from its own copy of the project. Your project folder is not touched.",
    );
    // No git words in what the person reads before deciding.
    for (const text of [approveHint(worker), approveHint(review("r4")), rejectHint(worker)]) {
      expect(text).not.toMatch(/checkout|branch|merg/i);
    }
    const partial = review("r5", { mode: "isolated", state: "partly_approved", not_copied: ["NOTES.md"] });
    expect(itemSummary(partial)).toBe("1 file, 1 not copied");
    expect(notCopiedHint(partial)).toContain("your version changed since the worker started: NOTES.md");
    const both = review("r6", {
      mode: "isolated",
      state: "partly_approved",
      files: [change("NOTES.md"), change("a.rs", { blocked: true })],
      not_copied: ["NOTES.md", "a.rs"],
    });
    expect(notCopiedHint(both)).toBe(
      "Not copied into your project folder, because your version changed since the worker started: NOTES.md; and because the direct agent had changed them too: a.rs. The worker's version of each is in its diff below. Replace yours with the worker's version (you can undo it), copy parts by hand, or keep yours and mark this done.",
    );
    expect(fileMarkers(change("a.rs", { blocked: true }), false, "partly_approved")).toEqual([
      "Not copied: it waited for the direct agent",
    ]);
    expect(rejectHint(review("r2", { files: [change("a", { by: "both" })] }))).toContain(
      "Changes made outside the agent stay as they are",
    );
    expect(defaultCommitMessage(worker)).toBe("Fix the typo");
    expect(defaultCommitMessage(review("r3"))).toBe("");
  });

  it("finds the files that agents of other open reviews changed too", () => {
    const a = review("a", { files: [change("x"), change("y"), change("z", { by: "outside" })] });
    const b = review("b", { files: [change("x", { by: "both" }), change("z")] });
    const done = review("c", { state: "approved", files: [change("y")] });
    const items = [a, b, done];
    expect([...sharedFiles(a, items)]).toEqual(["x"]);
    expect([...sharedFiles(b, items)]).toEqual(["x"]);
    expect([...sharedFiles(done, items)]).toEqual([]);
    expect(fileMarkers(change("x"), true)).toEqual(["Also changed in another open review"]);
    expect(itemSummary(a, 1)).toBe("3 files, 1 changed outside the agent, 1 also in another open review");
  });
});
