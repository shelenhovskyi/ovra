import { expect, test } from "vitest";
import { listCommands } from "../keys/commands";
import { registerEditorCommands } from "./commands";

test("editor.toggle switches the open file between viewing and editing", () => {
  const workspace: { openPath: string | null; mode: "view" | "edit" } = { openPath: null, mode: "view" };
  const unregister = registerEditorCommands(workspace);
  const toggle = listCommands().find((c) => c.id === "editor.toggle");
  void toggle?.run();
  expect(workspace.mode).toBe("view");
  workspace.openPath = "a.rs";
  void toggle?.run();
  expect(workspace.mode).toBe("edit");
  void toggle?.run();
  expect(workspace.mode).toBe("view");
  unregister();
  expect(listCommands().some((c) => c.id === "editor.toggle")).toBe(false);
});
