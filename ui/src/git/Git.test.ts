import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import { listCommands, registerCommand } from "../keys/commands";
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
/** The path of the focused file row, if a row has focus. */
const focusedRow = () => (document.activeElement as HTMLElement | null)?.dataset.path;
function press(code: string, init: KeyboardEventInit = {}): void {
  const key = { ArrowDown: "ArrowDown", End: "End", Escape: "Escape" }[code] ?? code.replace("Key", "").toLowerCase();
  (document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent("keydown", { code, key, bubbles: true, cancelable: true, ...init }),
  );
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
  expect(focusedRow()).toBe("new.md");

  click("[data-testid=git-staged] .head button");
  await settle();
  expect(names("staged")).toEqual([]);

  click("[aria-label='Open src/a.ts']");
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
  expect(focusedRow()).toBe("new.md");
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
  expect(focusedRow()).toBe("new.md");
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
  box.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", ctrlKey: true, bubbles: true }));
  flushSync();
  expect(text("[data-testid=git-announce]")).toBe("Stage changes to commit them, or amend the last commit.");

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

  // Amend starts from the last message, and unticking takes it out again.
  const amend = $<HTMLInputElement>("input[type=checkbox]");
  amend?.click();
  await settle();
  expect(box.value).toBe("Fix the thing");
  expect(text(".commit .hint")).toMatch(/first line/);
  amend?.click();
  await settle();
  expect(box.value).toBe("");
});

it("switches branches, shows why a switch failed and creates a branch", async () => {
  const core = new FakeGitCore();
  core.branches.push({ name: "topic", current: false, upstream: null, commit: null });
  await setup(core);
  click("[data-testid=git-branch]");
  await settle();
  expect(document.activeElement?.textContent?.trim()).toBe("main (current)");

  const refused = "you have changes to a.ts that this branch would overwrite: commit them or discard them first";
  core.failNext.set("git.switch", new CoreError({ code: "conflict", message: refused }));
  click("#git-branch-menu li:nth-child(2) button");
  await settle();
  expect(text("[data-testid=git-branch-error]")).toBe(
    "Could not switch to topic. You have changes to a.ts that this branch would overwrite: commit them or discard them first.",
  );

  // From the palette: the menu opens on a new branch's name.
  await listCommands()
    .find((c) => c.id === "git.new_branch")
    ?.run();
  await settle();
  const input = $<HTMLInputElement>("#git-branch-menu input");
  if (!input) throw new Error("no branch name");
  expect(document.activeElement).toBe(input);
  input.value = "my feature";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  expect(text("[data-testid=git-branch-name-problem]")).toBe("Branch names cannot contain spaces. Try my-feature.");
  $<HTMLFormElement>("#git-branch-menu form")?.requestSubmit();
  await settle();
  expect(core.methods()).not.toContain("git.create_branch");
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

  click("[data-testid=git-changes] [data-path='a.ts']");
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

  click("[data-testid=git-changes] [data-path='new.md']");
  await settle();
  expect($("[data-hunk-action]")).toBeNull();
  expect(text("[data-testid=git-diff-no-hunks]")).toMatch(/untracked/);
  // The diff is the selected tab, and its row says it is shown.
  expect(text("[role=tab][aria-selected=true]")).toBe("new.md diff");
  expect($("[data-path='new.md']")?.getAttribute("aria-current")).toBe("true");
  press("Escape");
  await settle();
  expect(workspace.gitDiff).toBeNull();
  expect($("[data-diff-tab]")).toBeNull();
  expect(focusedRow()).toBe("new.md");
});

it("is one Tab stop per list, with keys for row actions and the close-tab key for the diff", async () => {
  const off = registerCommand({ id: "editor.close_tab", title: "Close tab", keys: "Mod+W", run: () => {} });
  const core = new FakeGitCore();
  core.status = repo({
    files: [changed("a.ts"), changed("b.ts"), changed("c.ts", { staged: "modified", unstaged: null })],
  });
  await setup(core);
  const stops = [...target.querySelectorAll<HTMLElement>("[data-row]")].filter((r) => r.tabIndex === 0);
  expect(stops.map((r) => r.dataset.path)).toEqual(["c.ts", "a.ts"]);
  expect($("#git-changes-keys")?.textContent).toContain("S: stage.");

  $("[data-testid=git-changes] [data-path='a.ts']")?.focus();
  press("End");
  expect(focusedRow()).toBe("b.ts");
  press("KeyS");
  await settle();
  expect(core.calls.find((c) => c.method === "git.stage")?.params).toEqual({ project: "p1", paths: ["b.ts"] });
  expect(focusedRow()).toBe("a.ts");
  // Discard only asks; Escape goes back to the row.
  press("Delete");
  await settle();
  expect(document.activeElement?.textContent).toBe("Cancel");
  press("Escape");
  await settle();
  expect(focusedRow()).toBe("a.ts");
  expect(core.methods()).not.toContain("git.discard");

  click("[data-testid=git-changes] [data-path='a.ts']");
  await settle();
  press("KeyW", { altKey: true });
  await settle();
  expect(workspace.gitDiff).toBeNull();
  expect(focusedRow()).toBe("a.ts");
  off();
});
