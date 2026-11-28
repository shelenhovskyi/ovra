import { EditorSelection, EditorState } from "@codemirror/state";
import { flushSync } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { listCommands } from "../keys/commands";
import { installKeybindings } from "../keys/keybindings";
import type { Project } from "../lib/protocol";
import { mainView } from "../shell/main-view.svelte";
import { Workspace } from "../shell/workspace.svelte";
import { noteCursor, registerNavigation } from "./navigation.svelte";

const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0)) cleanup();
});

let projects = 0;
function setup(): Workspace {
  const workspace = new Workspace();
  const id = `nav-${++projects}`;
  workspace.project = { id, name: id, root: `/${id}`, last_opened_ms: null, trusted: false } satisfies Project;
  cleanups.push(registerNavigation(workspace));
  flushSync();
  return workspace;
}

const text = Array.from({ length: 200 }, (_, i) => `line ${i + 1}`).join("\n");
/** The editor reports its cursor at `line` and `column` of `path`. */
function cursorAt(path: string, line: number, column = 0, edited = false): void {
  const state = EditorState.create({ doc: text });
  const at = state.doc.line(line).from + column;
  noteCursor(path, state.update({ selection: EditorSelection.cursor(at) }).state, edited);
}

function open(workspace: Workspace, path: string, mode: "view" | "edit" = "view"): void {
  workspace.openFile(path, mode);
  flushSync();
}

const run = (id: string) =>
  listCommands()
    .find((c) => c.id === id)
    ?.run();

describe("navigation", () => {
  it("goes back to the file and mode shown before, then forward again", () => {
    const workspace = setup();
    open(workspace, "a.md");
    open(workspace, "b.ts", "edit");
    void run("editor.go_back");
    flushSync();
    expect([workspace.openPath, workspace.mode, workspace.reveal]).toEqual(["a.md", "view", null]);
    void run("editor.go_forward");
    flushSync();
    expect([workspace.openPath, workspace.mode]).toEqual(["b.ts", "edit"]);
  });

  it("goes back to the cursor before a far jump in the editor, and reopens the file there", () => {
    const workspace = setup();
    open(workspace, "a.ts", "edit");
    cursorAt("a.ts", 5);
    cursorAt("a.ts", 7, 3);
    // Go to line 150: far enough to be a place of its own.
    cursorAt("a.ts", 150);
    open(workspace, "b.ts", "edit");
    cursorAt("b.ts", 1);
    void run("editor.go_back");
    flushSync();
    expect(workspace.reveal).toEqual({ path: "a.ts", line: 150, from: 0, to: 0 });
    cursorAt("a.ts", 150);
    void run("editor.go_back");
    expect(workspace.reveal).toEqual({ path: "a.ts", line: 7, from: 3, to: 3 });
  });

  it("takes a file opened at a spot as one place, not the cursor it shows first", () => {
    const workspace = setup();
    open(workspace, "a.ts", "edit");
    cursorAt("a.ts", 1);
    workspace.openFile("b.ts", "edit", { line: 60, from: 3, to: 3 });
    flushSync();
    // The editor shows the buffer, then selects the spot and clears it.
    cursorAt("b.ts", 1);
    cursorAt("b.ts", 60, 3);
    workspace.reveal = null;
    flushSync();
    open(workspace, "c.ts");
    void run("editor.go_back");
    expect(workspace.reveal).toEqual({ path: "b.ts", line: 60, from: 3, to: 3 });
    void run("editor.go_back");
    expect(workspace.reveal).toEqual({ path: "a.ts", line: 1, from: 0, to: 0 });
  });

  it("does not take typing that moves the cursor far as a jump", () => {
    const workspace = setup();
    open(workspace, "a.ts", "edit");
    cursorAt("a.ts", 1);
    cursorAt("a.ts", 120, 0, true);
    open(workspace, "b.ts");
    void run("editor.go_back");
    expect(workspace.reveal).toEqual({ path: "a.ts", line: 120, from: 0, to: 0 });
    void run("editor.go_back");
    expect(workspace.openPath).toBe("a.ts");
  });

  it("keeps a history per project", () => {
    const workspace = setup();
    const first = workspace.project;
    open(workspace, "a.ts");
    open(workspace, "b.ts");
    workspace.project = { id: "nav-other", name: "other", root: "/other", last_opened_ms: null, trusted: false };
    workspace.openPath = null;
    flushSync();
    open(workspace, "c.ts");
    void run("editor.go_back");
    expect(workspace.openPath).toBe("c.ts");
    // As switching projects does: the file goes first.
    workspace.openPath = null;
    workspace.project = first;
    flushSync();
    void run("editor.go_back");
    expect(workspace.openPath).toBe("a.ts");
  });

  it("closes a view covering the files", () => {
    const workspace = setup();
    open(workspace, "a.ts");
    open(workspace, "b.ts");
    mainView.toggle("audit");
    void run("editor.go_back");
    expect([mainView.current, workspace.openPath]).toEqual([null, "a.ts"]);
  });

  it("goes back and forward on the mouse's side buttons and on Alt+Left and Alt+Right", () => {
    const workspace = setup();
    cleanups.push(installKeybindings(window, false, false));
    open(workspace, "a.ts");
    open(workspace, "b.ts");
    const mouse = (button: number) => {
      const down = new MouseEvent("mousedown", { button, bubbles: true, cancelable: true });
      const up = new MouseEvent("mouseup", { button, bubbles: true, cancelable: true });
      document.body.dispatchEvent(down);
      document.body.dispatchEvent(up);
      flushSync();
      return down.defaultPrevented && up.defaultPrevented;
    };
    expect(mouse(3)).toBe(true);
    expect(workspace.openPath).toBe("a.ts");
    expect(mouse(4)).toBe(true);
    expect(workspace.openPath).toBe("b.ts");
    expect(mouse(0)).toBe(false);
    const key = (code: string) =>
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", { key: code, code, altKey: true, bubbles: true, cancelable: true }),
      );
    key("ArrowLeft");
    expect(workspace.openPath).toBe("a.ts");
    key("ArrowRight");
    expect(workspace.openPath).toBe("b.ts");
  });
});
