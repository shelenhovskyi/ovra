import { describe, expect, it } from "vitest";
import {
  approveHint,
  defaultCommitMessage,
  fileMarkers,
  fileSummary,
  itemSummary,
  itemTitle,
  notCopiedHint,
  protectedHint,
  protectedReason,
  protectedTag,
  rejectHint,
  sharedFiles,
} from "./describe";
import { change, review } from "./fake-review";

describe("inbox wording", () => {
  it("marks files not changed by the agent's file tools and files waiting for the direct agent", () => {
    expect(fileMarkers(change("a"))).toEqual([]);
    expect(fileMarkers(change("a", { by: "outside" }))).toEqual(["Not changed by the agent's file tools"]);
    expect(fileMarkers(change("a", { by: "both", blocked: true }))).toEqual([
      "Also changed outside the agent's file tools",
      "Waits for the direct agent",
    ]);
    expect(fileSummary(change("b", { kind: "renamed", from: "a", additions: 0, deletions: 0 }))).toBe(
      "renamed from a, +0 −0",
    );
  });

  it("says in plain words what each protected file does", () => {
    const reason = (path: string, from: string | null = null) =>
      protectedReason(change(path, { protected: true, from }));
    expect(reason(".envrc")).toBe("runs commands when you enter the folder with direnv");
    expect(reason(".mcp.json")).toBe("tools the agent can start");
    expect(reason(".claude/settings.json")).toBe("the agent's settings and permissions");
    expect(reason("docs/CLAUDE.md")).toBe("instructions every agent reads");
    expect(reason("ui/AGENTS.md")).toBe("instructions every agent reads");
    expect(reason(".vscode/tasks.json")).toBe("editor tasks and containers that can run code");
    expect(reason(".devcontainer/devcontainer.json")).toBe("editor tasks and containers that can run code");
    // A rename away from a protected path is explained by where it came from.
    expect(reason("notes.md", "CLAUDE.md")).toBe("instructions every agent reads");
  });

  it("marks protected files first and says a worker's approve leaves them out", () => {
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
      "Can run code or change agent rules",
      "Not changed by the agent's file tools",
    ]);
    expect(itemSummary(item)).toBe("4 files, 1 waiting for the direct agent");
    expect(protectedTag(item)).toBe("3 files can run code or change agent rules");
    expect(protectedTag(review("r2"))).toBeNull();
    expect(protectedHint(item)).toBe(
      "These can run code or change agent rules: .claude/settings.json (the agent's settings and permissions), docs/CLAUDE.md (from CLAUDE.md) (instructions every agent reads), .mcp.json (tools the agent can start). Approve leaves them out of your project folder unless you include them. Waiting for the direct agent either way: .mcp.json.",
    );
    expect(protectedHint(review("r2"))).toBeNull();
    expect(itemSummary(review("r3", { files: [change(".envrc", { by: "outside", protected: true })] }))).toBe(
      "1 file, none by the agent's file tools",
    );
  });

  it("says which of a direct session's protected files Reject restores and which it leaves", () => {
    const mine = review("r1", {
      mode: "direct",
      files: [change("CLAUDE.md", { protected: true }), change("src/a.ts")],
    });
    expect(protectedHint(mine)).toBe(
      "These can run code or change agent rules, and are already in your project folder: CLAUDE.md (instructions every agent reads). Approve keeps them; Reject restores them.",
    );
    const mixed = review("r2", {
      mode: "direct",
      files: [
        change(".mcp.json", { protected: true }),
        change(".envrc", { protected: true, by: "outside" }),
        change(".claude/settings.json", { protected: true, by: "both" }),
      ],
    });
    expect(protectedHint(mixed)).toBe(
      "These can run code or change agent rules, and are already in your project folder: .mcp.json (tools the agent can start), .envrc (runs commands when you enter the folder with direnv), .claude/settings.json (the agent's settings and permissions). Approve keeps them. Reject restores .mcp.json: the agent changed it with its own file tools. Reject leaves .envrc, .claude/settings.json as they are: changed outside the agent's file tools (by you, another program or the agent's commands).",
    );
    // The person's own edit is never called the agent's.
    const theirs = review("r3", { mode: "direct", files: [change(".envrc", { protected: true, by: "outside" })] });
    expect(protectedHint(theirs)).not.toMatch(/Reject restores/);
  });

  it("titles items without saying Worker twice", () => {
    expect(itemTitle(review("r1", { mode: "isolated" }), "Worker docs")).toBe("Worker docs");
    expect(itemTitle(review("r1", { mode: "isolated" }), "Session s2")).toBe("Worker: Session s2");
    expect(itemTitle(review("r1"), "Fix the typo")).toBe("Direct agent: Fix the typo");
  });

  it("sums up an item and says what approve and reject do", () => {
    const worker = review("r1", {
      mode: "isolated",
      files: [change("a"), change("b", { blocked: true }), change("c", { by: "outside" })],
      summary: "Fix the typo\n\nDetails",
    });
    expect(itemSummary(worker)).toBe(
      "3 files, 1 changed outside the agent's file tools, 1 waiting for the direct agent",
    );
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
      files: [change("NOTES.md"), change("a.rs", { blocked: true }), change(".envrc", { protected: true })],
      not_copied: [".envrc", "NOTES.md", "a.rs"],
    });
    expect(notCopiedHint(both)).toBe(
      "Not copied into your project folder, because your version changed since the worker started: NOTES.md; and because the direct agent had changed them too: a.rs; and because they can run code or change agent rules: .envrc. The worker's version of each is in its diff below. Replace yours with the worker's version (you can undo it), copy parts by hand, or keep yours and mark this done.",
    );
    expect(fileMarkers(change("a.rs", { blocked: true }), false, "partly_approved")).toEqual([
      "Not copied: it waited for the direct agent",
    ]);
    expect(fileMarkers(change(".envrc", { protected: true }), false, "partly_approved")).toEqual([
      "Can run code or change agent rules",
      "Not copied: left out as protected, or your version changed",
    ]);
    expect(rejectHint(review("r2", { files: [change("a", { by: "both" })] }))).toBe(
      "Reject restores only the files the agent changed with its own file tools, as they were at the start of the session. Files changed any other way (by you, another program or the agent's commands) stay as they are and are listed for you.",
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
    expect(itemSummary(a, 1)).toBe("3 files, 1 changed outside the agent's file tools, 1 also in another open review");
  });
});
