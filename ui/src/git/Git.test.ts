import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { Hunk } from "../lib/protocol";
import { CoreError } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { changed, FakeGitCore, repo } from "./fake-git";
import TestHost from "./TestHost.svelte";

let app: Record<string, unknown> | null = null;
let target: HTMLElement;
let workspace: Workspace;

beforeEach(() => {
  target = document.body.appendChild(document.createElement("div"));
});

afterEach(() => {
  if (app) unmount(app);
  app = null;
  target.remove();
});

/** Lets the fake's replies (a few promises deep) and the DOM updates through. */
async function settle(): Promise<void> {
  for (let i = 0; i < 12; i++) {
    flushSync();
    await Promise.resolve();
  }
  // One task turn, for chains longer than the loop above; no fixed wait.
  await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
}

async function setup(core = new FakeGitCore()): Promise<FakeGitCore> {
  workspace = new Workspace();
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  app = mount(TestHost, { target, props: { transport: core, workspace } });
  await settle();
  return core;
}

const $ = <T extends HTMLElement = HTMLElement>(selector: string) => target.querySelector<T>(selector);
const text = (selector: string) => $(selector)?.textContent?.replace(/\s+/g, " ").trim();
const names = (section: string) =>
  [...target.querySelectorAll(`[data-testid=git-${section}] .name`)].map((n) =>
    n.textContent?.replace(/\s+/g, " ").trim(),
  );
function click(selector: string): void {
  const element = $(selector);
  if (!element) throw new Error(`nothing matches ${selector}`);
  element.click();
}

it("shows the branch and the lists, stages and unstages files and opens them", async () => {
  const core = new FakeGitCore();
  core.status = repo({
    upstream: "origin/main",
    ahead: 2,
    files: [
      changed("src/a.ts", { staged: "modified", unstaged: null }),
      changed("src/b.ts"),
      changed("new.md", { unstaged: "untracked" }),
      changed("c.ts", { unstaged: "conflicted" }),
    ],
    truncated: 3,
  });
  await setup(core);

  expect(text("[data-testid=git-branch]")).toBe("Branch: main ▾");
  expect(text(".upstream")).toBe("origin/main 2 ahead");
  expect(names("conflicts")).toEqual(["! c.ts , conflicted"]);
  expect(names("staged")).toEqual(["M a.ts src , modified"]);
  expect(names("changes")).toEqual(["M b.ts src , modified", "U new.md , untracked"]);
  expect(text("[data-testid=git-truncated]")).toBe("3 more changed files are not shown.");

  click("[aria-label='Stage src/b.ts']");
  await settle();
  expect(core.calls.find((c) => c.method === "git.stage")?.params).toEqual({ project: "p1", paths: ["src/b.ts"] });
  expect(names("staged")).toHaveLength(2);
  expect(text("[data-testid=git-announce]")).toBe("Staged src/b.ts.");
  // Focus moved to the row now in the pressed button's place.
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Stage new.md");

  click("[data-testid=git-staged] .head button");
  await settle();
  expect(names("staged")).toEqual([]);

  click("[data-testid=git-changes] .name");
  expect(workspace.openPath).toBe("src/a.ts");
});

it("says when the project is not a repository", async () => {
  const core = new FakeGitCore();
  core.status = null;
  await setup(core);
  expect(text("[data-testid=git-no-repo]")).toBe("This folder is not a git repository.");
  expect(target.textContent).toContain("git init");
});

it("discards only after a confirmation and offers Undo until the next discard", async () => {
  const core = new FakeGitCore();
  core.status = repo({ files: [changed("new.md", { unstaged: "untracked" }), changed("b.ts")] });
  await setup(core);

  click("[aria-label='Discard changes to new.md…']");
  await settle();
  expect(text(".confirm p")).toContain("The file is deleted.");
  expect(document.activeElement?.textContent).toBe("Cancel");
  document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true }));
  await settle();
  expect($(".confirm")).toBeNull();
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Discard changes to new.md…");
  expect(core.methods()).not.toContain("git.discard");

  click("[aria-label='Discard changes to new.md…']");
  flushSync();
  click("[data-testid=git-confirm-discard]");
  await settle();
  expect(names("changes")).toEqual(["M b.ts , modified"]);
  click("[data-testid=git-undo-discard]");
  await settle();
  expect(core.calls.find((c) => c.method === "git.undo_discard")?.params).toEqual({
    project: "p1",
    undo: "undo-1",
    paths: ["new.md"],
  });
  expect(names("changes")).toHaveLength(2);
  expect($("[data-testid=git-undo-discard]")).toBeNull();
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Discard changes to new.md…");
});

it("commits with Mod+Enter, says why it waits and shows a hook's output", async () => {
  const core = new FakeGitCore();
  core.status = repo({ files: [changed("a.ts")] });
  await setup(core);
  const box = $<HTMLTextAreaElement>("textarea");
  if (!box) throw new Error("no commit message");
  const commit = $("[data-testid=git-commit]");
  expect(commit?.getAttribute("aria-disabled")).toBe("true");
  expect(text("#git-commit-why")).toBe("Stage changes to commit them, or amend the last commit.");

  click("[aria-label='Stage a.ts']");
  await settle();
  box.value = "Fix the thing\n\nBody";
  box.dispatchEvent(new Event("input", { bubbles: true }));
  core.failNext.set("git.commit", new CoreError({ code: "conflict", message: "pre-commit: lint failed" }));
  box.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", ctrlKey: true, bubbles: true }));
  await settle();
  expect(text("[data-testid=git-commit-error] pre")).toBe("pre-commit: lint failed");
  expect(box.value).toBe("Fix the thing\n\nBody");

  click("[data-testid=git-commit]");
  await settle();
  expect(core.calls.at(-2)).toMatchObject({ method: "git.commit", params: { message: "Fix the thing\n\nBody" } });
  expect(text("[data-testid=git-committed]")).toBe("Committed c100000: Fix the thing");
  expect(box.value).toBe("");
});

it("switches branches, shows why a switch failed and creates a branch", async () => {
  const core = new FakeGitCore();
  core.branches.push({ name: "topic", current: false, upstream: null, commit: null });
  await setup(core);
  click("[data-testid=git-branch]");
  await settle();
  expect(document.activeElement?.textContent?.trim()).toBe("main (current)");

  core.failNext.set(
    "git.switch",
    new CoreError({ code: "conflict", message: "an agent session writes this checkout" }),
  );
  click("#git-branch-menu li:nth-child(2) button");
  await settle();
  expect(text("[data-testid=git-branch-error]")).toBe(
    "Could not switch to topic: an agent session writes this checkout",
  );

  click("#git-branch-menu .new");
  await settle();
  const input = $<HTMLInputElement>("#git-branch-menu input");
  if (!input) throw new Error("no branch name");
  expect(document.activeElement).toBe(input);
  input.value = "feature/x";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  $<HTMLFormElement>("#git-branch-menu form")?.requestSubmit();
  await settle();
  expect(core.calls.find((c) => c.method === "git.create_branch")?.params).toEqual({
    project: "p1",
    name: "feature/x",
    switch: true,
  });
  expect($("#git-branch-menu")).toBeNull();
  expect(text("[data-testid=git-branch]")).toBe("Branch: feature/x ▾");
  expect(document.activeElement).toBe($("[data-testid=git-branch]"));
});

it("shows a diff with hunk staging and reloads it on fs.git_changed", async () => {
  const core = new FakeGitCore();
  core.status = repo({ files: [changed("a.ts"), changed("new.md", { unstaged: "untracked" })] });
  const hunk: Hunk = {
    old_start: 3,
    old_lines: 1,
    new_start: 3,
    new_lines: 1,
    header: "fn main",
    lines: ["-old", "+new"],
  };
  core.diffs.set("a.ts", { path: "a.ts", from: null, binary: false, hunks: [hunk], truncated: false });
  core.diffs.set("new.md", {
    path: "new.md",
    from: null,
    binary: false,
    hunks: [{ ...hunk, lines: ["+hi"] }],
    truncated: false,
  });
  await setup(core);

  click("[aria-label='Show diff of a.ts']");
  await settle();
  expect(workspace.gitDiff).toEqual({ project: "p1", path: "a.ts", staged: false });
  expect(document.activeElement?.id).toBe("git-diff-title");
  expect([...target.querySelectorAll("[data-testid=git-diff] .line .text")].map((l) => l.textContent)).toEqual([
    "-old",
    "+new",
  ]);
  click("[data-hunk-action]");
  await settle();
  expect(core.calls.find((c) => c.method === "git.stage_hunk")?.params).toEqual({
    project: "p1",
    path: "a.ts",
    hunk,
    unstage: false,
  });

  const diffs = () => core.methods().filter((m) => m === "git.diff").length;
  const before = diffs();
  core.emit({ type: "fs.git_changed", data: { project: "p1" } });
  await settle();
  expect(diffs()).toBe(before + 1);

  click("[aria-label='Show diff of new.md']");
  await settle();
  expect($("[data-hunk-action]")).toBeNull();
  expect(text("[data-testid=git-diff-no-hunks]")).toMatch(/untracked/);
});
