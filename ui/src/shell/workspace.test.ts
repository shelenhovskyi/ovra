import { describe, expect, test } from "vitest";
import { groupView, Workspace } from "./workspace.svelte";

describe("editor groups in the workspace", () => {
  test("openPath and mode are the focused group's; focusing swaps them", () => {
    const workspace = new Workspace();
    workspace.openFile("a.ts", "edit");
    workspace.split({ openPath: "a.ts", mode: "view" });
    expect([workspace.group, workspace.openPath, workspace.mode]).toEqual([1, "a.ts", "view"]);
    workspace.openFile("b.ts");
    workspace.focusGroup(0);
    expect([workspace.group, workspace.openPath, workspace.mode]).toEqual([0, "a.ts", "edit"]);
    expect(workspace.other).toEqual({ openPath: "b.ts", mode: "view" });
    // A third split only moves focus.
    workspace.split({ openPath: "c.ts", mode: "view" });
    expect(workspace.other).toEqual({ openPath: "b.ts", mode: "view" });
  });

  test("closing a group leaves the other as group 0, focused", () => {
    const workspace = new Workspace();
    workspace.openFile("a.ts");
    workspace.split({ openPath: "b.ts", mode: "edit" });
    workspace.closeGroup(1);
    expect([workspace.group, workspace.openPath, workspace.mode, workspace.other]).toEqual([0, "a.ts", "view", null]);
    workspace.split({ openPath: "b.ts", mode: "edit" });
    workspace.closeGroup(0);
    expect([workspace.group, workspace.openPath, workspace.mode, workspace.other]).toEqual([0, "b.ts", "edit", null]);
  });

  test("a group's view reads and writes that group, and opening there focuses it", () => {
    const workspace = new Workspace();
    workspace.openFile("a.ts");
    workspace.split({ openPath: "b.ts", mode: "view" });
    const left = groupView(workspace, 0);
    const right = groupView(workspace, 1);
    expect([left.openPath, right.openPath, left.focused, right.focused]).toEqual(["a.ts", "b.ts", false, true]);
    left.mode = "edit";
    expect([workspace.other?.mode, workspace.mode]).toEqual(["edit", "view"]);
    left.openFile("c.ts", "view");
    expect([workspace.group, workspace.openPath, right.openPath, left.openPath]).toEqual([0, "c.ts", "b.ts", "c.ts"]);
    // Everything else is the workspace's own.
    right.sidebar = "git";
    expect(workspace.sidebar).toBe("git");
  });
});
