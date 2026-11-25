import { flushSync, mount, unmount } from "svelte";
import { afterAll, afterEach, beforeAll, expect, it } from "vitest";
import { listCommands, registerCommand } from "../keys/commands";
import type { Hunk } from "../lib/protocol";
import { CoreError, type Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { changed, FakeGitCore, repo } from "./fake-git";
import TestHost from "./TestHost.svelte";

// One host (panel, tab strip, tabpanel) for the whole file, mounted once with
// the project open: each test gets a fresh panel and model by opening the
// project again, on its own fake core.
let core = new FakeGitCore();
const transport: Transport = {
  call: (request) => core.call(request),
  onEvent: (listener) => core.onEvent(listener),
};
const workspace = new Workspace();
const target = document.createElement("div");
let app: Record<string, unknown>;

beforeAll(async () => {
  document.body.append(target);
  app = mount(TestHost, { target, props: { transport, workspace } });
  await setup();
});

afterAll(() => {
  unmount(app);
  target.remove();
});

afterEach(() => {
  workspace.gitDiff = null;
  workspace.project = null;
  workspace.openPath = null;
  flushSync();
});

/** Lets the fake's replies (a few promises deep) and the DOM updates through, without a task turn. */
async function settle(): Promise<void> {
  for (let i = 0; i < 24; i++) {
    flushSync();
    await Promise.resolve();
  }
  flushSync();
}

async function setup(fake = new FakeGitCore()): Promise<FakeGitCore> {
  workspace.project = null;
  flushSync();
  core = fake;
  workspace.project = { id: "p1", name: "demo", root: "/demo", last_opened_ms: null, trusted: false };
  await settle();
  return fake;
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
  const key =
    { ArrowDown: "ArrowDown", ArrowUp: "ArrowUp", End: "End", Escape: "Escape", Enter: "Enter" }[code] ??
    code.replace("Key", "").toLowerCase();
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

  expect(text("[data-testid=git-branch]")).toBe("Branch: main");
  expect(text(".upstream")).toBe("origin/main 2 ahead");
  expect(names("conflicts")).toEqual(["! c.ts , conflicted"]);
  // A file in both lists has two rows with different names: one reads "staged".
  expect(names("staged")).toEqual(["M a.ts src , modified, staged"]);
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
  expect(text("[data-testid=git-no-repo] h2")).toBe("Not a git repository");
  expect(target.textContent).toContain("git init");

  // Then a repository on a detached HEAD, then one with no commits: plain words, no amend.
  core.status = repo({ branch: null });
  core.emit({ type: "fs.git_changed", data: { project: "p1" } });
  await settle();
  expect(text("[data-testid=git-branch]")).toBe("Branch: No branch, at commit abc1234 (detached HEAD)");
  expect(text("[data-testid=git-remote-hint]")).toBe("Push or pull in a terminal");
  // With no commits and no changes there is nothing to commit or list: one line.
  core.status = repo({ head: null });
  core.emit({ type: "fs.git_changed", data: { project: "p1" } });
  await settle();
  expect(text("[data-testid=git-fresh] h2")).toBe("No changes");
  expect($("[data-testid=git-commit]")).toBeNull();
  expect($("[data-testid=git-changes]")).toBeNull();
  core.status = repo({ head: null, files: [changed("a.ts", { unstaged: "untracked" })] });
  core.emit({ type: "fs.git_changed", data: { project: "p1" } });
  await settle();
  expect(text("#git-commit-why")).toBe("Stage files first.");
  expect(text("[data-testid=git-staged] .empty")).toBe("Nothing staged.");
});

it("discards only after a confirmation and offers Undo until the next discard", async () => {
  const core = new FakeGitCore();
  core.status = repo({ files: [changed("new.md", { unstaged: "untracked" }), changed("b.ts")] });
  await setup(core);
  // Drawn icons, with Discard last, away from Stage.
  const row = $("[data-testid=git-changes] [data-path='new.md']")?.parentElement;
  const actions = [...(row?.querySelectorAll<HTMLButtonElement>("button.icon") ?? [])];
  expect(actions.map((b) => b.getAttribute("aria-label"))).toEqual([
    "Open new.md",
    "Stage new.md",
    "Discard changes to new.md…",
  ]);
  expect(actions.every((b) => b.querySelector("svg") && b.textContent?.trim() === "")).toBe(true);

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
  expect(text("#git-commit-why")).toBe("Stage files, or amend.");
  box.dispatchEvent(new KeyboardEvent("keydown", { code: "Enter", ctrlKey: true, bubbles: true }));
  await settle();
  // Said once, under the button, and read out from there.
  expect(text("[data-testid=git-announce]")).toBe("");
  expect($("#git-commit-why")?.closest("[role=status]")).not.toBeNull();
  expect(text("#git-commit-why")).toBe("Stage files, or amend.");

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
  expect(text("[data-testid=git-announce]")).toBe("");
  expect(box.value).toBe("");
  // Recent commits read with the short id only: no title with the full one.
  click(".log .toggle");
  await settle();
  const entry = $("[data-testid=git-log] li");
  expect(entry?.hasAttribute("title")).toBe(false);
  expect(text("[data-testid=git-log] li .meta")).toMatch(/^c100000 · Ada · /);

  // Amend with an empty message keeps the last commit's message.
  $<HTMLInputElement>("input[type=checkbox]")?.click();
  await settle();
  expect(box.value).toBe("");
  expect(text(".commit .hint")).toMatch(/empty message keeps/);
  // Mod+Enter commits from anywhere in the box, here the checkbox.
  $<HTMLInputElement>("input[type=checkbox]")?.focus();
  press("Enter", { ctrlKey: true });
  await settle();
  expect(core.calls.filter((c) => c.method === "git.commit").at(-1)).toMatchObject({
    method: "git.commit",
    params: { message: "", amend: true },
  });
});

it("switches branches, shows why a switch failed and creates a branch", async () => {
  const core = new FakeGitCore();
  core.branches.push(
    { name: "topic", current: false, upstream: null, commit: null },
    { name: "fix/login", current: false, upstream: null, commit: null },
  );
  await setup(core);
  click("[data-testid=git-branch]");
  await settle();
  const focused = () => document.activeElement?.textContent?.trim();
  expect(focused()).toBe("main (current)");
  // Arrows, Home and End move; a letter goes on in the filter, Enter there switches to the first match.
  press("End");
  expect(focused()).toBe("fix/login");
  press("Home");
  press("ArrowDown");
  expect(focused()).toBe("topic");
  press("KeyF");
  flushSync();
  const filter = $<HTMLInputElement>("#git-branch-menu [data-filter]");
  expect(document.activeElement).toBe(filter);
  expect(filter?.value).toBe("f");
  expect(text("[data-testid=git-branch-matches]")).toBe("1 of 3 branches. Enter switches to fix/login.");
  press("Escape");
  flushSync();
  expect(filter?.value).toBe("");
  expect($("#git-branch-menu")).not.toBeNull();
  press("ArrowDown");
  expect(focused()).toBe("main (current)");
  // New branch, then Cancel: back on "New branch…", and Escape closes the menu.
  click("#git-branch-menu .new");
  await settle();
  [...target.querySelectorAll<HTMLElement>("#git-branch-menu form button")].at(-1)?.click();
  await settle();
  expect(focused()).toBe("New branch…");
  press("Escape");
  await settle();
  expect($("#git-branch-menu")).toBeNull();
  click("[data-testid=git-branch]");
  await settle();

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
  const input = $<HTMLInputElement>("#git-branch-menu form input");
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
  expect(text("[data-testid=git-branch]")).toBe("Branch: feature/x");
  expect(document.activeElement).toBe($("[data-testid=git-branch]"));
});

it("shows a diff with hunk staging and reloads it on fs.git_changed", async () => {
  const core = new FakeGitCore();
  core.status = repo({
    files: [changed("a.ts", { staged: "modified" }), changed("new.md", { unstaged: "untracked" })],
  });
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
  // Said in words: a leading sign is often not read out.
  const spoken = (line: Element) => {
    const copy = line.cloneNode(true) as Element;
    for (const hidden of copy.querySelectorAll("[aria-hidden=true]")) hidden.remove();
    return copy.textContent?.replace(/\s+/g, " ").trim();
  };
  expect([...target.querySelectorAll("[data-testid=git-diff] .line")].map(spoken)).toEqual([
    "removed line 3: old",
    "added line 3: new",
  ]);
  // A hunk is a group with a plain title, not a landmark; the @@ header is its tooltip.
  const title = $("[data-testid=git-diff] [role=group][aria-label='Hunk 1 of 1, lines 3 to 3'] h3");
  expect(title?.textContent?.replace(/\s+/g, " ").trim()).toBe("Lines 3 to 3 fn main");
  expect(title?.title).toBe("@@ -3,1 +3,1 @@");
  expect($("[data-testid=git-diff] section.hunk")).toBeNull();
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

  // Switched to the staged side: Escape still goes back to the row it was opened from.
  click("[data-testid=git-changes] [data-path='a.ts']");
  await settle();
  [...target.querySelectorAll<HTMLElement>("[data-testid=git-diff] button")]
    .find((b) => b.textContent === "Show staged changes")
    ?.click();
  await settle();
  expect(workspace.gitDiff).toEqual({ project: "p1", path: "a.ts", staged: true });
  press("Escape");
  await settle();
  expect(document.activeElement?.closest("[data-testid=git-changes]")).not.toBeNull();
  expect(focusedRow()).toBe("a.ts");
});

it("follows the file's changes to the staged side, and says where they went after the commit", async () => {
  const core = new FakeGitCore();
  core.status = repo({ files: [changed("a.ts")] });
  const diff = (lines: string[]) => ({
    path: "a.ts",
    from: null,
    binary: false,
    hunks: [{ old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, header: "", lines }],
    truncated: false,
  });
  core.diffs.set("a.ts", diff(["-old", "+new"]));
  await setup(core);
  click("[data-testid=git-changes] [data-path='a.ts']");
  await settle();

  core.diffs.delete("a.ts");
  core.diffs.set("staged:a.ts", diff(["-old", "+new"]));
  $<HTMLElement>("[data-testid=git-changes] [data-path='a.ts']")?.focus();
  click("[aria-label='Stage a.ts']");
  await settle();
  expect(workspace.gitDiff).toEqual({ project: "p1", path: "a.ts", staged: true });
  // Focus stays in the panel, where the person works.
  expect(document.activeElement?.closest("[data-testid=git]")).not.toBeNull();
  expect(text("[data-testid=git-diff] [role=status]")).toBe("All changes in this file are staged: showing them.");

  core.diffs.delete("staged:a.ts");
  const box = $<HTMLTextAreaElement>("textarea");
  if (box) box.value = "Say hello";
  box?.dispatchEvent(new Event("input", { bubbles: true }));
  click("[data-testid=git-commit]");
  await settle();
  expect(text("[data-testid=git-diff-empty]")).toBe(
    "No changes left in this file: they were committed as c100000 (Say hello).",
  );
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
  // The keys show under the list while one of its rows has focus.
  expect($<HTMLElement>("[data-testid=git-changes-keys]")?.hidden).toBe(true);
  $("[data-testid=git-changes] [data-path='a.ts']")?.focus();
  flushSync();
  expect($<HTMLElement>("[data-testid=git-changes-keys]")?.hidden).toBe(false);
  expect(text("[data-testid=git-changes-keys]")).toContain("S: stage.");
  // A key that does nothing here says why.
  press("KeyU");
  flushSync();
  expect(text("[data-testid=git-announce]")).toBe("This file is not staged: there is nothing to unstage.");

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
