import { afterEach, expect, test } from "vitest";
import { listCommands } from "../keys/commands";
import type { Transport } from "../lib/transport";
import { Workspace } from "../shell/workspace.svelte";
import { Buffers } from "./buffers.svelte";
import { registerEditorCommands } from "./commands";
import { Drafts } from "./drafts.svelte";
import { OpenFiles, tabs } from "./open-files.svelte";

const transport = { call: async () => ({ type: "ok" }), onEvent: () => () => {} } as unknown as Transport;

afterEach(() => {
  tabs.current = null;
});

function run(id: string): void {
  void listCommands()
    .find((c) => c.id === id)
    ?.run();
}

test("editor.toggle switches the open file between viewing and editing", () => {
  const workspace = new Workspace();
  const unregister = registerEditorCommands(workspace);
  run("editor.toggle");
  expect(workspace.mode).toBe("view");
  workspace.openFile("a.rs");
  run("editor.toggle");
  expect(workspace.mode).toBe("edit");
  run("editor.toggle");
  expect(workspace.mode).toBe("view");
  unregister();
  expect(listCommands().some((c) => c.id.startsWith("editor."))).toBe(false);
});

test("tab commands switch, wrap around and close, keeping the mode", () => {
  const workspace = new Workspace();
  const unregister = registerEditorCommands(workspace);
  const files = new OpenFiles(
    transport,
    "p1",
    new Buffers({ transport, project: "p1", drafts: new Drafts(null), highlighter: () => null }),
  );
  tabs.current = files;
  for (const path of ["a", "b", "c"]) void files.opened(path);
  workspace.openFile("c", "edit");

  run("editor.next_tab");
  expect([workspace.openPath, workspace.mode]).toEqual(["a", "edit"]);
  void files.opened("a");
  run("editor.previous_tab");
  expect(workspace.openPath).toBe("c");
  void files.opened("c");

  run("editor.close_tab");
  expect(files.files).toEqual(["a", "b"]);
  expect(workspace.openPath).toBe("b");
  unregister();
});
