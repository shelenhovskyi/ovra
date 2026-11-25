// Two editor groups (docs/specs/split-editor.md): opening, moving and
// closing tabs between them, and what the project state keeps of them.

import { flushSync } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { Project } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { Drafts } from "./drafts.svelte";
import { FakeCore, MemoryStorage } from "./fake-core";
import {
  forgetTabs,
  moveTab,
  OpenFiles,
  persistOpenFiles,
  renameTabs,
  requestClose,
  splitRight,
  tabs,
} from "./open-files.svelte";

const project: Project = { id: "p1", name: "p", root: "/p", last_opened_ms: null, trusted: false };

afterEach(() => {
  tabs.current = null;
  tabs.closing = null;
  tabs.focused = () => 0;
});

async function setup(open: string[]) {
  const core = new FakeCore();
  for (const path of open) core.put(path, `text of ${path}`);
  const drafts = new Drafts(new MemoryStorage());
  const buffers = new Buffers({ transport: core, project: "p1", drafts, highlighter: () => null });
  const files = new OpenFiles(core, "p1", buffers);
  const workspace = new Workspace();
  tabs.current = files;
  tabs.focused = () => workspace.group;
  for (const path of open) {
    workspace.openFile(path);
    await files.opened(path);
    files.makePermanent(path);
  }
  /** Waits for the state writes so far. */
  const written = () => files.opened(files.active);
  return { core, buffers, files, workspace, written };
}

test("split right opens the file in a new focused group; a second split moves focus", async () => {
  const { core, files, workspace, written } = await setup(["a.ts", "b.ts"]);
  splitRight(workspace);
  const right = tabs.groups[1];
  expect([workspace.group, workspace.openPath, workspace.other?.openPath]).toEqual([1, "b.ts", "b.ts"]);
  expect([tabs.current, right?.files, right?.preview]).toEqual([right, ["b.ts"], null]);
  await written();
  expect(core.state.second_group).toEqual({ open_files: ["b.ts"], active_file: "b.ts", pinned_files: [] });
  splitRight(workspace);
  expect([workspace.group, tabs.current, tabs.groups.length]).toEqual([0, files, 2]);
});

test("one text for a file in both groups: closing one tab keeps it, without asking", async () => {
  const { buffers, files, workspace } = await setup(["a.ts"]);
  splitRight(workspace);
  const buffer = buffers.open("a.ts");
  await buffer.loaded;
  buffer.edit({ changes: { from: 0, insert: "unsaved " } });
  requestClose(workspace, "a.ts");
  // The right group was the last tab's: it closes, and the left one takes the whole width.
  expect([tabs.closing, tabs.groups, workspace.other, workspace.group]).toEqual([null, [files], null, 0]);
  expect([buffers.get("a.ts"), buffer.text]).toEqual([buffer, "unsaved text of a.ts"]);
  expect(workspace.openPath).toBe("a.ts");
});

test("moving a tab makes a group for it, and moving the last one back closes that group", async () => {
  const { core, buffers, files, workspace, written } = await setup(["a.ts", "b.ts"]);
  const b = buffers.open("b.ts");
  await b.loaded;
  b.edit({ changes: { from: 0, insert: "!" } });
  moveTab(workspace, "b.ts", 1);
  const right = tabs.groups[1];
  expect([files.files, right?.files, workspace.group, workspace.openPath]).toEqual([["a.ts"], ["b.ts"], 1, "b.ts"]);
  expect(workspace.other).toEqual({ openPath: "a.ts", mode: "view" });
  expect([buffers.get("b.ts"), b.dirty]).toEqual([b, true]);
  // The only tab of a single group stays where it is.
  moveTab(workspace, "a.ts", 0);
  expect(tabs.groups).toHaveLength(2);
  moveTab(workspace, "b.ts", 0);
  expect([tabs.groups, files.files, workspace.other, workspace.openPath]).toEqual([
    [files],
    ["a.ts", "b.ts"],
    null,
    "b.ts",
  ]);
  await written();
  expect([core.state.open_files, core.state.second_group]).toEqual([["a.ts", "b.ts"], undefined]);
});

test("when the left group closes, the right one becomes the first and is written as it", async () => {
  const { core, workspace, written } = await setup(["a.ts", "b.ts"]);
  moveTab(workspace, "b.ts", 1);
  workspace.focusGroup(0);
  requestClose(workspace, "a.ts");
  const [only] = tabs.groups;
  expect([tabs.groups.length, only?.slot, workspace.group, workspace.openPath]).toEqual([1, 0, 0, "b.ts"]);
  await written();
  expect([core.state.open_files, core.state.active_file, core.state.second_group]).toEqual([
    ["b.ts"],
    "b.ts",
    undefined,
  ]);
});

test("renames and deletions reach the tabs of both groups", async () => {
  const { files, workspace } = await setup(["src/a.ts", "b.ts"]);
  splitRight(workspace);
  workspace.focusGroup(0);
  renameTabs(workspace, "b.ts", "c.ts");
  expect([files.files, tabs.groups[1]?.files, workspace.openPath, workspace.other?.openPath]).toEqual([
    ["src/a.ts", "c.ts"],
    ["c.ts"],
    "c.ts",
    "c.ts",
  ]);
  forgetTabs(workspace, ["c.ts"]);
  expect([tabs.groups, files.files, workspace.openPath, workspace.other]).toEqual([
    [files],
    ["src/a.ts"],
    "src/a.ts",
    null,
  ]);
});

test("a split main area comes back on reopen, and an older state opens one group", async () => {
  const core = new FakeCore();
  core.state = {
    open_files: ["a.ts"],
    active_file: "a.ts",
    expanded_dirs: [],
    second_group: { open_files: ["b.ts", "c.ts"], active_file: "gone.ts", pinned_files: ["c.ts"] },
  };
  const workspace = new Workspace();
  const stop = persistOpenFiles(core, workspace);
  workspace.project = project;
  flushSync();
  await vi.waitFor(() => expect(tabs.groups).toHaveLength(2));
  const right = tabs.groups[1];
  expect([workspace.group, workspace.openPath, workspace.other]).toEqual([
    0,
    "a.ts",
    { openPath: "c.ts", mode: "view" },
  ]);
  expect([right?.files, right?.pinned, right?.slot]).toEqual([["c.ts", "b.ts"], ["c.ts"], 1]);
  // Another project, saved before the split editor, opens with one group.
  core.state = { open_files: ["x.ts"], active_file: "x.ts", expanded_dirs: [] };
  workspace.openPath = null;
  workspace.project = { ...project, id: "p2" };
  flushSync();
  await vi.waitFor(() => expect(workspace.openPath).toBe("x.ts"));
  expect([tabs.groups.length, workspace.other]).toEqual([1, null]);
  stop();
});
