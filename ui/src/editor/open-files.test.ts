import { flushSync } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { Project } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { Drafts } from "./drafts.svelte";
import { FakeCore, MemoryStorage } from "./fake-core";
import { modeMemory } from "./mode-memory";
import { answerClose, modeOf, OpenFiles, persistOpenFiles, requestClose, show, tabs } from "./open-files.svelte";

const project: Project = { id: "p1", name: "p", root: "/p", last_opened_ms: null, trusted: false };

afterEach(() => {
  tabs.current = null;
  tabs.closing = null;
});

function setup() {
  const core = new FakeCore();
  const drafts = new Drafts(new MemoryStorage());
  const buffers = new Buffers({ transport: core, project: "p1", drafts, highlighter: () => null });
  const files = new OpenFiles(core, "p1", buffers);
  return { core, drafts, buffers, files };
}

test("writes only its own fields and keeps the tab order", async () => {
  const { core, files } = setup();
  core.state = { open_files: ["a.rs", "b.rs"], active_file: "b.rs", expanded_dirs: ["src"] };
  expect(await files.restore()).toBe("b.rs");
  await files.opened("a.rs");
  expect(core.sentStates.at(-1)).toEqual({ open_files: ["a.rs", "b.rs"], active_file: "a.rs", pinned_files: [] });
  await core.call({ method: "projects.set_state", params: { id: "p1", state: { expanded_dirs: ["docs"] } } });
  await files.opened(null);
  expect(core.state).toEqual({
    open_files: ["a.rs", "b.rs"],
    active_file: null,
    expanded_dirs: ["docs"],
    pinned_files: [],
  });
});

test("keep adds a tab at the end without making it active", async () => {
  const { core, files } = setup();
  core.state = { open_files: ["a"], active_file: "a", expanded_dirs: [] };
  await files.restore();
  files.keep("b");
  files.keep("a");
  await files.opened("a");
  expect(core.state.open_files).toEqual(["a", "b"]);
  expect(files.active).toBe("a");
  expect(files.modes.get("b")).toBe("edit");
});

test("closing the active tab activates its right neighbour, else its left", async () => {
  const { core, files } = setup();
  core.state = { open_files: ["a", "b", "c"], active_file: "b", expanded_dirs: [] };
  await files.restore();
  expect(files.close("b")).toBe("c");
  expect(files.close("a")).toBe("c");
  expect(files.close("c")).toBeNull();
  await files.opened(null);
  expect(core.state.open_files).toEqual([]);
});

test("the tab limit closes the leftmost clean tab, never one with unsaved text", async () => {
  const { core, drafts, files } = setup();
  await files.restore();
  drafts.set("p1", "f0", { text: "draft", mtime: 1 });
  for (let i = 0; i < 20; i++) {
    void files.opened(`f${i}`);
    files.makePermanent(`f${i}`);
  }
  await files.opened("f20");
  expect(files.files).toHaveLength(20);
  expect(files.files.slice(0, 2)).toEqual(["f0", "f2"]);
  expect(core.state.open_files).toEqual(files.files);
});

test("closing a tab with unsaved text asks first: cancel, don't save, save", async () => {
  const { core, buffers, files } = setup();
  core.put("a.txt", "one");
  core.put("b.txt", "two");
  await files.restore();
  tabs.current = files;
  const workspace = new Workspace();
  workspace.project = project;
  for (const path of ["a.txt", "b.txt"]) {
    workspace.openFile(path, "edit");
    await files.opened(path);
    files.makePermanent(path);
  }
  const b = buffers.open("b.txt");
  await b.loaded;
  // Typing happens in the view; a draft stands in for unsaved text here.
  buffers.open("a.txt");
  await buffers.get("a.txt")?.loaded;

  requestClose(workspace, "a.txt");
  expect(tabs.closing).toBeNull();
  expect(files.files).toEqual(["b.txt"]);

  vi.spyOn(b, "dirty", "get").mockReturnValue(true);
  requestClose(workspace, "b.txt");
  expect(tabs.closing).toBe("b.txt");
  await answerClose(workspace, "cancel");
  expect([tabs.closing, files.files]).toEqual([null, ["b.txt"]]);

  requestClose(workspace, "b.txt");
  const saving = answerClose(workspace, "save");
  // The question stays until the file is written; a second answer meanwhile does nothing.
  expect([tabs.closing, tabs.saving]).toEqual(["b.txt", true]);
  await answerClose(workspace, "discard");
  expect(files.files).toEqual(["b.txt"]);
  await saving;
  expect([tabs.closing, tabs.saving]).toEqual([null, false]);
  expect(core.writes.at(-1)).toEqual({ path: "b.txt", text: "two", expected: 102 });
  expect(files.files).toEqual([]);
  expect(workspace.openPath).toBeNull();
});

test("a failed save keeps the tab open and shows it", async () => {
  const { core, buffers, files } = setup();
  core.put("a.txt", "one");
  await files.restore();
  tabs.current = files;
  const workspace = new Workspace();
  await files.opened("a.txt");
  const a = buffers.open("a.txt");
  await a.loaded;
  vi.spyOn(a, "dirty", "get").mockReturnValue(true);
  core.put("a.txt", "changed by someone else");
  requestClose(workspace, "a.txt");
  await answerClose(workspace, "save");
  expect(files.files).toEqual(["a.txt"]);
  expect([a.conflict, workspace.openPath, workspace.mode]).toEqual([true, "a.txt", "edit"]);
});

test("reopens the active file when a project opens, then records changes", async () => {
  const core = new FakeCore();
  core.state = { open_files: ["main.rs"], active_file: "main.rs", expanded_dirs: [] };
  const workspace = new Workspace();
  const stop = persistOpenFiles(core, workspace);
  workspace.project = project;
  flushSync();
  await vi.waitFor(() => expect(workspace.openPath).toBe("main.rs"));
  workspace.openFile("lib.rs");
  flushSync();
  await vi.waitFor(() => expect(core.state.active_file).toBe("lib.rs"));
  expect(core.state.open_files).toEqual(["main.rs", "lib.rs"]);
  stop();
});

test("each tab keeps its own mode; a file without a tab opens in View", async () => {
  const core = new FakeCore();
  core.state = { open_files: [], active_file: null, expanded_dirs: [] };
  const workspace = new Workspace();
  const stop = persistOpenFiles(core, workspace);
  workspace.project = project;
  flushSync();
  await vi.waitFor(() => expect(tabs.current).not.toBeNull());
  workspace.openFile("main.rs", "edit");
  flushSync();
  tabs.current?.makePermanent("main.rs");
  workspace.openFile("lib.rs", modeOf("lib.rs"));
  flushSync();
  expect(workspace.mode).toBe("view");
  show(workspace, "main.rs");
  expect(workspace.mode).toBe("edit");
  show(workspace, "lib.rs");
  expect(workspace.mode).toBe("view");
  stop();
});

test("a file opened without a tab is the preview, replaced in place by the next until kept", async () => {
  const { core, drafts, files } = setup();
  core.state = { open_files: ["a"], active_file: "a", expanded_dirs: [] };
  await files.restore();
  await files.opened("b");
  expect([files.files, files.preview]).toEqual([["a", "b"], "b"]);
  files.modes.set("b", "edit");
  await files.opened("a");
  await files.opened("c");
  expect([files.files, files.preview, files.modes.has("b")]).toEqual([["a", "c"], "c", false]);
  files.makePermanent("c");
  await files.opened("d");
  expect([files.files, files.preview]).toEqual([["a", "c", "d"], "d"]);
  // A preview with unsaved text is never replaced.
  drafts.set("p1", "d", { text: "draft", mtime: 1 });
  await files.opened("e");
  expect([files.files, files.preview]).toEqual([["a", "c", "d", "e"], "e"]);
  files.close("e");
  expect(files.preview).toBeNull();
});

test("pinned tabs come first and stay through close others and close all", async () => {
  const { core, drafts, files } = setup();
  const open_files = ["a", "b", "c", "d", "e"];
  core.state = { open_files, active_file: "e", expanded_dirs: [], pinned_files: ["c", "gone"] };
  await files.restore();
  expect([files.files, files.pinned]).toEqual([["c", "a", "b", "d", "e"], ["c"]]);
  files.togglePin("b");
  expect(files.files).toEqual(["c", "b", "a", "d", "e"]);
  drafts.set("p1", "d", { text: "draft", mtime: 1 });
  expect(files.closeOthers("a")).toBe(1);
  expect([files.files, files.active]).toEqual([["c", "b", "a", "d"], "a"]);
  expect(files.closeOthers(null)).toBe(1);
  expect([files.files, files.active]).toEqual([["c", "b", "d"], "c"]);
  files.togglePin("c");
  await files.opened("b");
  expect([files.files, files.pinned]).toEqual([["b", "c", "d"], ["b"]]);
  expect(core.state).toMatchObject({ open_files: ["b", "c", "d"], pinned_files: ["b"] });
  files.close("b");
  expect(files.pinned).toEqual([]);
});

test("typing in the preview tab keeps it; a kind of file opens the way it was last switched to", async () => {
  const core = new FakeCore();
  core.put("a.zz", "one");
  const workspace = new Workspace();
  const stop = persistOpenFiles(core, workspace);
  workspace.project = project;
  flushSync();
  await vi.waitFor(() => expect(tabs.current).not.toBeNull());
  workspace.openFile("a.zz");
  flushSync();
  const files = tabs.current as OpenFiles;
  expect(files.preview).toBe("a.zz");
  const a = files.buffers.open("a.zz");
  await a.loaded;
  a.dirty = true;
  flushSync();
  expect(files.preview).toBeNull();
  expect(modeOf("b.zz")).toBe("view");
  modeMemory.set("x.zz", "edit");
  expect(modeOf("b.zz")).toBe("edit");
  stop();
});
