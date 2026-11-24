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

test("editor.toggle on a binary file stays in view mode and says why", async () => {
  const { core, workspace, unregister } = setup();
  core.files.set("logo.png", { body: { kind: "binary", mime: "image/png", base64: "" }, mtime: 1 });
  workspace.openFile("logo.png");
  await run("editor.toggle");
  expect(workspace.mode).toBe("view");
  expect(tabs.notice).toBe("logo.png is a binary file and cannot be edited here.");
  unregister();
});

test("tab commands switch, wrap around and close, keeping the mode", async () => {
  const { files, workspace, unregister } = setup();
  for (const path of ["a", "b", "c"]) void files.opened(path);
  workspace.openFile("c", "edit");

  await run("editor.next_tab");
  expect([workspace.openPath, workspace.mode]).toEqual(["a", "edit"]);
  void files.opened("a");
  await run("editor.previous_tab");
  expect(workspace.openPath).toBe("c");
  void files.opened("c");

  await run("editor.close_tab");
  expect(files.files).toEqual(["a", "b"]);
  expect(workspace.openPath).toBe("b");
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
