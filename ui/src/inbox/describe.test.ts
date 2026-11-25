import { describe, expect, it } from "vitest";
import {
  approveDetail,
  approveHint,
  defaultCommitMessage,
  fileMarkers,
  fileSummary,
  gitTag,
  gitWarning,
  itemSummary,
  itemTitle,
  keptFirst,
  notCopiedHint,
  protectedHint,
  protectedReason,
  protectedTag,
  rejectHint,
  sharedFiles,
  sharedHint,
  shortTitle,
  taskOf,
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
    expect(reason("sub/.envrc")).toBe("runs commands when you enter the folder with direnv");
    expect(reason("a/b/.claude/settings.json")).toBe("the agent's settings and permissions");
    expect(reason("docs/GEMINI.md")).toBe("instructions every agent reads");
    expect(reason(".cursor/rules")).toBe("another editor's or agent's settings and rules");
    expect(reason(".husky/pre-commit")).toBe("git hooks: run when you commit or push");
    expect(reason(".githooks/pre-push")).toBe("git hooks: run when you commit or push");
    expect(reason("src/.gitattributes")).toBe("tells git which programs to run on files");
    expect(reason(".gitmodules")).toBe("other repositories git fetches into this one");
    expect(reason(".lfsconfig")).toBe("where git downloads large files from");
    expect(reason("odd/name")).toBe("can run code or change agent rules");
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
      "These can run code or change agent rules: .claude/settings.json (the agent's settings and permissions), docs/CLAUDE.md (from CLAUDE.md) (instructions every agent reads), .mcp.json (tools the agent can start). Keep changes leaves them out of your project folder unless you include them. Waiting for the direct agent either way: .mcp.json.",
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
      "These can run code or change agent rules, and are already in your project folder: CLAUDE.md (instructions every agent reads). Keep changes leaves them; Reject restores them.",
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
      "These can run code or change agent rules, and are already in your project folder: .mcp.json (tools the agent can start), .envrc (runs commands when you enter the folder with direnv), .claude/settings.json (the agent's settings and permissions). Keep changes leaves them. Reject restores .mcp.json: the agent changed it with its own file tools. Reject leaves .envrc, .claude/settings.json as they are: changed outside the agent's file tools (by you, another program or the agent's commands).",
    );
    // The person's own edit is never called the agent's.
    const theirs = review("r3", { mode: "direct", files: [change(".envrc", { protected: true, by: "outside" })] });
    expect(protectedHint(theirs)).not.toMatch(/Reject restores/);
  });

  it("warns about git hooks and config that no diff shows and nothing undoes", () => {
    const item = review("r1", {
      files: [],
      git_changes: [
        { path: ".git/hooks/pre-commit", kind: "added" },
        { path: "/home/me/.gitconfig", kind: "modified" },
      ],
    });
    expect(gitTag(item)).toBe("2 git hooks or config files changed");
    expect(itemSummary(item)).toBe("No changed files");
    const warning = gitWarning(item) ?? "";
    expect(warning).toContain(".git/hooks/pre-commit (added), /home/me/.gitconfig (modified)");
    expect(warning).toContain("Keep changes and Reject leave them as they are");
    expect(warning).toContain("check each one yourself");
    expect(warning).not.toContain("could not check");

    const unchecked = review("r2", { git_unchecked: true });
    expect(gitTag(unchecked)).toBe("Git hooks and config not checked");
    expect(gitWarning(unchecked)).toBe(
      "Ovra could not check git hooks and config for this item: look at them yourself.",
    );
    expect(gitTag(review("r3"))).toBeNull();
    expect(gitWarning(review("r3"))).toBeNull();
    expect(gitTag(review("r4", { git_changes: [{ path: ".husky/_/pre-commit", kind: "deleted" }] }))).toBe(
      "1 git hook or config file changed",
    );
  });

  it("titles items without saying Worker twice", () => {
    expect(itemTitle(review("r1", { mode: "isolated" }), "Worker docs")).toBe("Worker docs");
    expect(itemTitle(review("r1", { mode: "isolated" }), "Session s2")).toBe("Worker: Session s2");
    expect(itemTitle(review("r1", { mode: "isolated" }), "lint · Add notes · v2")).toBe("lint (task: Add notes · v2)");
    expect(itemTitle(review("r1"), "Fix the typo")).toBe("Direct agent: Fix the typo");
    // The list says the worker's name under its task, once.
    const lint = review("r1", { mode: "isolated" });
    expect([shortTitle(lint, "lint · Add notes"), taskOf(lint, "lint · Add notes")]).toEqual(["lint", "Add notes"]);
    expect([shortTitle(review("r1"), "Fix it"), taskOf(review("r1"), "a · b")]).toEqual(["Fix it", null]);
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
    expect(approveDetail(worker)).toContain("A file you changed yourself since the worker started is not copied");
    expect(approveHint(review("r4"))).toBe("Keep changes leaves them in your project folder.");
    expect(approveDetail(review("r4"))).toBeNull();
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
    // A rename away from a protected path leaves both paths, for the same reason.
    const moved = review("r7", {
      mode: "isolated",
      state: "partly_approved",
      files: [change("notes.md", { kind: "renamed", from: "CLAUDE.md", protected: true })],
      not_copied: ["CLAUDE.md", "notes.md"],
    });
    expect(notCopiedHint(moved)).toContain("because they can run code or change agent rules: CLAUDE.md, notes.md.");
    expect(notCopiedHint(moved)).not.toContain("your version changed");
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

  it("finds the files that workers of other open reviews changed too", () => {
    const isolated = "isolated" as const;
    const a = review("a", { mode: isolated, files: [change("x"), change("y"), change("z", { by: "outside" })] });
    const b = review("b", { mode: isolated, files: [change("x", { by: "both" }), change("z")] });
    const done = review("c", { mode: isolated, state: "approved", files: [change("y")] });
    // A direct agent's files are in place already, so its item marks every file a worker changed too;
    // the worker's file that waits for the direct agent says so itself.
    const direct = review("d", { files: [change("y"), change("w")] });
    const waits = review("e", { mode: isolated, files: [change("w", { blocked: true })] });
    const items = [a, b, done, direct, waits];
    expect([...sharedFiles(a, items)]).toEqual(["x"]);
    expect([...sharedFiles(b, items)]).toEqual(["x"]);
    expect([...sharedFiles(done, items)]).toEqual([]);
    expect([...sharedFiles(direct, items)]).toEqual(["y", "w"]);
    expect(sharedHint(direct, items, (o) => o.id)).toBeNull();
    expect([...sharedFiles(waits, items)]).toEqual([]);
    expect(fileMarkers(change("x"), true)).toEqual(["Also changed in another open review"]);
    expect(sharedHint(a, items, (o) => `${o.id} · task`)).toBe(
      "b also changed x. Only the version you keep first is copied; a later one that differs is left for you to choose, and one that is the same counts as copied.",
    );
    expect(itemSummary(a)).toBe("3 files, 1 changed outside the agent's file tools");
  });

  it("says once a worker's version of a shared file is kept, instead of the shared marker", () => {
    const docs = review("docs", { mode: "isolated", files: [change("NOTES.md")] });
    const lint = review("lint", { mode: "isolated", files: [change("NOTES.md")] });
    const tests = review("tests", { mode: "isolated", files: [change("NOTES.md")] });
    expect([...sharedFiles(lint, [docs, lint, tests])]).toEqual(["NOTES.md"]);
    const kept = { ...docs, state: "approved" as const };
    const items = [kept, lint, tests];
    expect([...sharedFiles(lint, items)]).toEqual([]);
    expect(keptFirst(lint, items).get("NOTES.md")?.id).toBe("docs");
    expect(fileMarkers(change("NOTES.md"), false, null, null, "docs")).toEqual(["docs's version is already kept"]);
    // Not copied by docs: nothing was kept first.
    expect(keptFirst(lint, [{ ...kept, not_copied: ["NOTES.md"] }, lint]).size).toBe(0);
  });
});
