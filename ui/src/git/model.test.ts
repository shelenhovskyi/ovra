import { describe, expect, it, vi } from "vitest";
import { CoreError } from "../lib/transport";
import { changed, FakeGitCore, repo } from "./fake-git";
import {
  branchNameProblem,
  FETCH_AFTER_FAILURE_MS,
  FETCH_AGAIN_MS,
  FETCH_EVERY_MS,
  fileLists,
  GitModel,
  gitModel,
  SLOW_MS,
} from "./model.svelte";

async function loaded(core = new FakeGitCore(), project = "p1") {
  const model = new GitModel(core, project);
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

  it("fetches the upstream on open, now and then and on coming back, quietly", async () => {
    vi.useFakeTimers();
    try {
      const core = new FakeGitCore();
      core.status = repo({ upstream: "origin/main" });
      core.fetchFinds = 2;
      const { model } = await loaded(core);
      await vi.advanceTimersByTimeAsync(0);
      const fetches = () => core.calls.filter((c) => c.method === "git.fetch").length;
      expect(fetches()).toBe(1);
      // What the fetch found shows as new commits.
      expect(model.state.kind === "ready" && model.state.status.behind).toBe(2);
      // Back to the window soon after: no second fetch yet.
      window.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
      expect(fetches()).toBe(1);
      await vi.advanceTimersByTimeAsync(FETCH_AGAIN_MS);
      window.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
      expect(fetches()).toBe(2);
      // A failed fetch says nothing, and the next waits longer (a sign-in it could not ask for).
      core.failNext.set("git.fetch", new CoreError({ code: "unavailable", message: "no network" }));
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(fetches()).toBe(3);
      expect(model.error).toBeNull();
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      window.dispatchEvent(new Event("focus"));
      await vi.advanceTimersByTimeAsync(0);
      expect(fetches()).toBe(3);
      await vi.advanceTimersByTimeAsync(FETCH_AFTER_FAILURE_MS - FETCH_EVERY_MS);
      expect(fetches()).toBe(4);

      // "Get commits" waits for a fetch under way, so the fetch cannot undo what it records.
      let release = () => {};
      core.holdNext.set("git.fetch", (r) => {
        release = r;
      });
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(fetches()).toBe(5);
      const pulled = model.pull(2);
      await vi.advanceTimersByTimeAsync(0);
      expect(core.methods()).not.toContain("git.pull");
      release();
      expect(await pulled).toBe(true);
      expect(core.methods()).toContain("git.pull");
      model.dispose();
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(fetches()).toBe(5);

      // No upstream: nothing to ask.
      const local = await loaded();
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(local.core.methods()).not.toContain("git.fetch");
      local.model.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it("holds other actions while a sync waits for a fetch, and counts what the fetch found", async () => {
    vi.useFakeTimers();
    try {
      const core = new FakeGitCore();
      core.status = repo({
        upstream: "origin/main",
        behind: 2,
        files: [changed("a.ts", { staged: "modified", unstaged: null })],
      });
      let release = () => {};
      core.holdNext.set("git.fetch", (r) => {
        release = r;
      });
      const { model } = await loaded(core, "sync-wait");
      await vi.advanceTimersByTimeAsync(0);
      expect(core.methods()).toContain("git.fetch");
      core.fetchFinds = 3;
      const first = model.pull(2);
      const second = model.pull(2);
      expect(model.busy).toBe(true);
      expect(await second).toBe(false);
      expect(await model.commit("Fix", false)).toBe(false);
      expect(model.syncing).toBe("pull");
      release();
      expect(await first).toBe(true);
      expect(core.methods().filter((m) => m === "git.pull")).toHaveLength(1);
      expect(core.methods()).not.toContain("git.commit");
      expect(model.got).toBe(3);
      expect(model.notice).toBe("Got 3 commits.");
      model.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it("backs off fetching only when the remote is out of reach, also across a new model", async () => {
    vi.useFakeTimers();
    try {
      const core = new FakeGitCore();
      core.status = repo({ upstream: "origin/main" });
      const fetched = () => core.methods().filter((m) => m === "git.fetch").length;
      // The branch changed meanwhile: the next fetch need not wait.
      core.failNext.set("git.fetch", new CoreError({ code: "conflict", message: "the branch changed meanwhile" }));
      const { model } = await loaded(core, "backoff");
      await vi.advanceTimersByTimeAsync(0);
      expect(fetched()).toBe(1);
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(fetched()).toBe(2);

      core.failNext.set("git.fetch", new CoreError({ code: "unavailable", message: "no network" }));
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(fetched()).toBe(3);
      model.dispose();
      // Another project shown and this one again: no fetch at once.
      const again = await loaded(core, "backoff");
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(fetched()).toBe(3);
      // On another branch the remote may answer.
      core.status = repo({ branch: "dev", upstream: "origin/dev" });
      again.model.changed();
      await vi.advanceTimersByTimeAsync(FETCH_EVERY_MS);
      expect(fetched()).toBe(4);
      again.model.dispose();
    } finally {
      vi.useRealTimers();
    }
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
      message: "The commit was not made:",
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

it("offers Cancel for a commit that keeps running, and says it was stopped", async () => {
  vi.useFakeTimers();
  try {
    const core = new FakeGitCore();
    core.status = repo({ files: [changed("a.ts", { staged: "modified", unstaged: null })] });
    const { model } = await loaded(core);
    let release = () => {};
    core.holdNext.set("git.commit", (r) => {
      release = r;
    });
    const commit = model.commit("Fix", false);
    vi.advanceTimersByTime(SLOW_MS - 1);
    expect(model.slow).toBeNull();
    vi.advanceTimersByTime(1);
    expect(model.slow).toBe("commit");

    await model.cancel();
    expect(core.calls.at(-1)).toEqual({ method: "git.cancel", params: { project: "p1" } });
    expect(model.stopping).toBe(true);
    core.failNext.set("git.commit", new CoreError({ code: "conflict", message: "git commit was stopped" }));
    release();
    expect(await commit).toBe(false);
    expect(model.error).toEqual({
      scope: "commit",
      message: "You stopped the commit.",
      output: "git commit was stopped",
    });
    expect([model.slow, model.stopping, model.busy]).toEqual([null, false, false]);

    // A quick commit never shows it.
    expect(await model.commit("Again", true)).toBe(true);
    vi.advanceTimersByTime(SLOW_MS);
    expect(model.slow).toBeNull();
    model.dispose();
  } finally {
    vi.useRealTimers();
  }
});

it("says why a branch name is refused, with a fix for spaces", () => {
  expect(branchNameProblem("my feature")).toBe("Branch names cannot contain spaces. Try my-feature.");
  expect(branchNameProblem("a..b")).toMatch(/two dots/);
  expect(branchNameProblem("feature/")).toMatch(/end with \//);
  expect(branchNameProblem("x.lock")).toMatch(/\.lock/);
  expect(branchNameProblem("feature/login-2")).toBeNull();
});
