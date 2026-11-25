import { flushSync } from "svelte";
import { afterEach, expect, test, vi } from "vitest";
import type { Project } from "../lib/protocol";
import { Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { Drafts } from "./drafts.svelte";
import { FakeCore, MemoryStorage } from "./fake-core";
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
  expect(core.sentStates.at(-1)).toEqual({ open_files: ["a.rs", "b.rs"], active_file: "a.rs" });
  await core.call({ method: "projects.set_state", params: { id: "p1", state: { expanded_dirs: ["docs"] } } });
  await files.opened(null);
  expect(core.state).toEqual({ open_files: ["a.rs", "b.rs"], active_file: null, expanded_dirs: ["docs"] });
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
  for (let i = 0; i < 20; i++) void files.opened(`f${i}`);
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
  workspace.openFile("lib.rs", modeOf("lib.rs"));
  flushSync();
  expect(workspace.mode).toBe("view");
  show(workspace, "main.rs");
  expect(workspace.mode).toBe("edit");
  show(workspace, "lib.rs");
  expect(workspace.mode).toBe("view");
  stop();
});
