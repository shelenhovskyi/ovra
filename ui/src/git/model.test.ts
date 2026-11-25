import { describe, expect, it } from "vitest";
import { CoreError } from "../lib/transport";
import { changed, FakeGitCore, repo } from "./fake-git";
import { branchNameProblem, fileLists, GitModel, gitModel } from "./model.svelte";

async function loaded(core = new FakeGitCore()) {
  const model = new GitModel(core, "p1");
  // The first status call is the constructor's.
  for (let i = 0; i < 4; i++) await Promise.resolve();
  return { core, model };
}

describe("GitModel", () => {
  it("loads the status, and says when the folder is not a repository or git is missing", async () => {
    const { model } = await loaded();
    expect(model.state).toEqual({ kind: "ready", status: repo() });
    expect(gitModel("p1")).toBe(model);
    model.dispose();
    expect(gitModel("p1")).toBeUndefined();

    const none = new FakeGitCore();
    none.status = null;
    expect((await loaded(none)).model.state).toEqual({ kind: "no_repo" });

    const missing = new FakeGitCore();
    missing.failNext.set("git.status", new CoreError({ code: "unavailable", message: "git is not installed" }));
    expect((await loaded(missing)).model.state).toEqual({
      kind: "error",
      message: "Git is not available here: git is not installed",
    });
  });

  it("reloads once for a burst of git_changed and file events of its project only", async () => {
    const { core, model } = await loaded();
    core.calls = [];
    core.emit({ type: "fs.git_changed", data: { project: "other" } });
    core.emit({ type: "fs.changed", data: { project: "other", paths: ["a"] } });
    for (let i = 0; i < 4; i++) core.emit({ type: "fs.git_changed", data: { project: "p1" } });
    core.emit({ type: "fs.changed", data: { project: "p1", paths: ["a.ts"] } });
    await model.refresh();
    expect(core.methods()).toEqual(["git.status", "git.status"]);
    expect(model.version).toBe(5);
    model.dispose();
  });

  it("stages, unstages, discards with undo, and refreshes after each", async () => {
    const core = new FakeGitCore();
    core.status = repo({ files: [changed("a.ts"), changed("new.md", { unstaged: "untracked" })] });
    const { model } = await loaded(core);

    await model.stage(["a.ts"]);
    let lists = fileLists(model.state.kind === "ready" ? model.state.status.files : []);
    expect(lists.staged.map((f) => f.path)).toEqual(["a.ts"]);
    expect(model.notice).toBe("Staged a.ts.");

    await model.discard(["new.md"]);
    expect(model.discarded).toEqual({ undo: "undo-1", paths: ["new.md"] });
    lists = fileLists(model.state.kind === "ready" ? model.state.status.files : []);
    expect(lists.changes).toEqual([]);

    await model.undoDiscard();
    expect(model.discarded).toBeNull();
    lists = fileLists(model.state.kind === "ready" ? model.state.status.files : []);
    expect(lists.changes.map((f) => f.path)).toEqual(["new.md"]);
    expect(core.methods().filter((m) => m !== "git.status")).toEqual(["git.stage", "git.discard", "git.undo_discard"]);
    expect(core.methods().at(-1)).toBe("git.status");
    model.dispose();
  });

  it("keeps a hook's output when a commit fails and names a conflicting hunk", async () => {
    const core = new FakeGitCore();
    core.status = repo({ files: [changed("a.ts", { staged: "modified", unstaged: null })] });
    const { model } = await loaded(core);
    core.failNext.set("git.commit", new CoreError({ code: "conflict", message: "lint failed\nsrc/a.ts:1" }));
    expect(await model.commit("Fix", false)).toBe(false);
    expect(model.error).toEqual({
      scope: "commit",
      message: "The commit was not made. Git or a hook refused it:",
      output: "lint failed\nsrc/a.ts:1",
    });

    expect(await model.commit("Fix it", false)).toBe(true);
    expect(model.error).toBeNull();
    // The commit box says it, once.
    expect(model.committed?.commit.summary).toBe("Fix it");
    expect(model.notice).toBe("");

    core.failNext.set("git.stage_hunk", new CoreError({ code: "conflict", message: "patch does not apply" }));
    const hunk = { old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, header: "", lines: ["-a", "+b"] };
    await model.stageHunk("a.ts", hunk, false);
    expect(model.error?.scope).toBe("diff");
    expect(model.error?.message).toMatch(/changed since this diff/);
    model.dispose();
  });
});

it("says why a branch name is refused, with a fix for spaces", () => {
  expect(branchNameProblem("my feature")).toBe("Branch names cannot contain spaces. Try my-feature.");
  expect(branchNameProblem("a..b")).toMatch(/two dots/);
  expect(branchNameProblem("feature/")).toMatch(/end with \//);
  expect(branchNameProblem("x.lock")).toMatch(/\.lock/);
  expect(branchNameProblem("feature/login-2")).toBeNull();
});
