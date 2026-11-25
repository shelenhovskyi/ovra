import { EditorView } from "@codemirror/view";
import { afterEach, expect, test, vi } from "vitest";
import { listCommands } from "../keys/commands";
import { Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { registerEditorCommands } from "./commands";
import { Drafts } from "./drafts.svelte";
import { FakeCore, MemoryStorage } from "./fake-core";
import { OpenFiles, tabs, unsavedText } from "./open-files.svelte";

afterEach(() => {
  tabs.current = null;
  tabs.notice = null;
});

function setup() {
  const core = new FakeCore();
  const drafts = new Drafts(new MemoryStorage());
  const files = new OpenFiles(
    core,
    "p1",
    new Buffers({ transport: core, project: "p1", drafts, highlighter: () => null }),
  );
  tabs.current = files;
  const workspace = new Workspace();
  const unregister = registerEditorCommands(workspace);
  return { core, drafts, files, workspace, unregister };
}

async function run(id: string): Promise<void> {
  await listCommands()
    .find((c) => c.id === id)
    ?.run();
}

test("editor.toggle switches a text file between viewing and editing", async () => {
  const { core, workspace, unregister } = setup();
  await run("editor.toggle");
  expect(workspace.mode).toBe("view");
  core.put("a.rs", "fn a() {}");
  workspace.openFile("a.rs");
  await run("editor.toggle");
  expect(workspace.mode).toBe("edit");
  await run("editor.toggle");
  expect(workspace.mode).toBe("view");
  unregister();
  expect(listCommands().some((c) => c.id.startsWith("editor."))).toBe(false);
});

test("editor.keep_tab keeps the preview tab, so the next file opens beside it", async () => {
  const { files, unregister } = setup();
  void files.opened("a");
  expect(files.preview).toBe("a");
  await run("editor.keep_tab");
  expect(files.preview).toBeNull();
  void files.opened("b");
  expect(files.files).toEqual(["a", "b"]);
  unregister();
});

test("editor.toggle on a binary file stays in view mode and says why", async () => {
  const { core, workspace, unregister } = setup();
  core.files.set("logo.png", { body: { kind: "binary", mime: "image/png", base64: "" }, mtime: 1 });
  workspace.openFile("logo.png");
  await run("editor.toggle");
  expect(workspace.mode).toBe("view");
  expect(tabs.notice).toBe("logo.png is a binary file and cannot be edited here.");
  unregister();
});

test("tab commands switch, wrap around and close, each tab in its own mode", async () => {
  const { files, workspace, unregister } = setup();
  for (const path of ["a", "b", "c"]) {
    void files.opened(path);
    files.makePermanent(path);
  }
  workspace.openFile("c", "edit");
  files.modes.set("c", "edit");

  await run("editor.next_tab");
  expect([workspace.openPath, workspace.mode]).toEqual(["a", "view"]);
  void files.opened("a");
  await run("editor.previous_tab");
  expect([workspace.openPath, workspace.mode]).toEqual(["c", "edit"]);
  void files.opened("c");

  await run("editor.close_tab");
  expect(files.files).toEqual(["a", "b"]);
  expect(workspace.openPath).toBe("b");
  unregister();
});

test("pin, close other tabs and close all tabs keep pinned and unsaved tabs, and say why", async () => {
  const { drafts, files, workspace, unregister } = setup();
  for (const path of ["a", "b", "c", "d"]) {
    void files.opened(path);
    files.makePermanent(path);
  }
  workspace.openFile("b");
  await files.opened("b");
  await run("editor.toggle_pin");
  expect([files.files, files.pinned]).toEqual([["b", "a", "c", "d"], ["b"]]);
  drafts.set("p1", "d", { text: "draft", mtime: 1 });
  workspace.openFile("a");
  await files.opened("a");
  await run("editor.close_other_tabs");
  expect([files.files, tabs.notice]).toEqual([["b", "a", "d"], "A tab with unsaved changes stayed open."]);
  await run("editor.close_all_tabs");
  expect([files.files, workspace.openPath]).toEqual([["b", "d"], "b"]);
  unregister();
});

test("close tabs to the right and close saved tabs keep pinned and unsaved tabs", async () => {
  const { drafts, files, workspace, unregister } = setup();
  const open = async (path: string) => {
    workspace.openFile(path);
    await files.opened(path);
    files.makePermanent(path);
  };
  for (const path of ["a", "b", "c", "d", "e"]) await open(path);
  files.togglePin("e");
  // The active tab closing hands over to the one the menu was opened on.
  await open("d");
  files.closeToTheRight("b");
  expect([files.files, files.active]).toEqual([["e", "a", "b"], "b"]);
  for (const path of ["c", "d"]) await open(path);
  drafts.set("p1", "c", { text: "draft", mtime: 1 });
  await run("editor.close_tabs_to_the_right");
  expect(files.files).toEqual(["e", "a", "b", "c", "d"]);
  await open("b");
  await run("editor.close_tabs_to_the_right");
  expect([files.files, tabs.notice]).toEqual([["e", "a", "b", "c"], "A tab with unsaved changes stayed open."]);
  await run("editor.close_saved_tabs");
  expect([files.files, workspace.openPath, tabs.notice]).toEqual([["e", "c"], "e", null]);
  unregister();
});

test("unsavedText gives the viewer a draft or the buffer's unsaved text", async () => {
  const { core, drafts, files } = setup();
  drafts.set("p1", "draft.md", { text: "# draft", mtime: 1 });
  expect(unsavedText("p1", "draft.md")).toBe("# draft");
  expect(unsavedText("other", "draft.md")).toBeNull();
  core.put("clean.md", "# clean");
  const buffer = files.buffers.open("clean.md");
  await buffer.loaded;
  expect(unsavedText("p1", "clean.md")).toBeNull();
  vi.spyOn(buffer, "unsaved", "get").mockReturnValue("# edited");
  expect(unsavedText("p1", "clean.md")).toBe("# edited");
});

test("editor.save in view mode saves unsaved text and otherwise does nothing", async () => {
  const { core, drafts, workspace, unregister } = setup();
  core.put("a.txt", "disk");
  workspace.openFile("a.txt");
  await run("editor.save");
  expect(core.writes).toEqual([]);
  drafts.set("p1", "a.txt", { text: "draft", mtime: 101 });
  await run("editor.save");
  expect(core.writes).toEqual([{ path: "a.txt", text: "draft", expected: 101 }]);
  expect(drafts.has("p1", "a.txt")).toBe(false);
  unregister();
});

test("git change commands in view mode switch the file to editing and then act", async () => {
  const { core, files, workspace, unregister } = setup();
  core.put("a.rs", "fn a() {}");
  workspace.openFile("a.rs");
  const done = run("editor.next_git_change");
  const buffer = files.buffers.open("a.rs");
  await buffer.loaded;
  expect(workspace.mode).toBe("edit");
  // What Editor.svelte does in edit mode; the command then acts in that view.
  const view = new EditorView({ parent: document.body });
  buffer.attach(view);
  await done;
  // The fake core has no git, so there is no change to go to, and it says so.
  expect(view.dom.querySelector(".cm-announced")?.textContent).toBe("No git changes in this file.");
  buffer.detach();
  view.destroy();
  unregister();
});

test("a language action from the viewer opens the file for editing, then runs in the editor", async () => {
  const { core, files, workspace, unregister } = setup();
  const action = vi.fn();
  files.lsp = { action } as unknown as NonNullable<typeof files.lsp>;
  core.put("a.rs", "fn a() {}");
  workspace.openFile("a.rs");
  const done = run("editor.lsp_definition");
  const buffer = files.buffers.open("a.rs");
  await buffer.loaded;
  expect(workspace.mode).toBe("edit");
  const view = new EditorView({ parent: document.body });
  buffer.attach(view);
  await done;
  expect(action).toHaveBeenCalledWith("definition");
  expect(listCommands().find((c) => c.id === "editor.lsp_references")?.keys).toBe("Shift+F12");
  buffer.detach();
  view.destroy();
  unregister();
});

test("find, replace and go to line have VS Code's keys, which no other editor command uses", () => {
  const { unregister } = setup();
  const commands = listCommands().filter((c) => c.id.startsWith("editor."));
  const keys = (id: string) => commands.find((c) => c.id === id)?.keys;
  expect([keys("editor.find"), keys("editor.replace"), keys("editor.go_to_line")]).toEqual([
    "Mod+F",
    "Mod+Alt+F",
    "Ctrl+G",
  ]);
  const all = commands.flatMap((c) => (c.keys ? [c.keys] : []));
  expect(new Set(all).size).toBe(all.length);
  unregister();
});

test("reopen closed tab brings back the last closed files, newest first, skipping deleted ones", async () => {
  const { core, files, workspace, unregister } = setup();
  for (const path of ["a", "b", "c", "d"]) {
    core.put(path, path);
    await files.opened(path);
    files.makePermanent(path);
  }
  files.close("a");
  files.closeOthers("d");
  expect(files.files).toEqual(["d"]);
  core.files.delete("c");
  await run("editor.reopen_closed_tab");
  expect([workspace.openPath, files.files, files.preview]).toEqual(["b", ["d", "b"], null]);
  await run("editor.reopen_closed_tab");
  expect(workspace.openPath).toBe("a");
  workspace.openFile("d");
  await run("editor.reopen_closed_tab");
  expect(workspace.openPath).toBe("d");
  unregister();
});

test("save all writes every unsaved file and names the ones it could not save", async () => {
  const { core, drafts, files, unregister } = setup();
  for (const path of ["a", "b", "c"]) {
    core.put(path, path);
    await files.opened(path);
    files.makePermanent(path);
  }
  drafts.set("p1", "a", { text: "a2", mtime: 101 });
  // A draft against an older version of b is a conflict, never overwritten.
  drafts.set("p1", "b", { text: "b2", mtime: 1 });
  await run("editor.save_all");
  expect(core.writes).toEqual([{ path: "a", text: "a2", expected: 101 }]);
  expect([files.buffers.get("b")?.conflict, tabs.notice]).toEqual([true, "Not saved, open to see why: b."]);
  unregister();
});
