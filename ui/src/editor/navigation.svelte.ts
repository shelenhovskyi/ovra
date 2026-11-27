// Go back and Go forward in the window: the shown file and mode come from the
// workspace, the cursor from the focused group's editor (`noteCursor`), each
// into the open project's history (./navigation.ts), kept in memory.

import type { EditorState } from "@codemirror/state";
import { untrack } from "svelte";
import { registerCommand } from "../keys/commands";
import { isApple } from "../keys/shortcut";
import type { ProjectId } from "../lib/protocol";
import { mainView } from "../shell/main-view.svelte";
import type { Workspace } from "../shell/workspace.svelte";
import { NavigationHistory, type Place } from "./navigation";

/**
 * VS Code's keys: Ctrl+- and Ctrl+Shift+- on Apple keyboards (Cmd+- zooms
 * out there), Alt+Left and Alt+Right elsewhere, which the editor leaves to
 * these commands (buffers.svelte.ts). A browser tab's Back and Forward are
 * the same keys, so headless mode needs no stand-ins.
 */
export const BACK_KEYS = isApple() ? "Ctrl+-" : "Alt+ArrowLeft";
export const FORWARD_KEYS = isApple() ? "Ctrl+Shift+-" : "Alt+ArrowRight";

/** Where the editor's cursor goes: the open project's history, in the window registered last. */
let report: ((place: Place, edited: boolean) => void) | null = null;

/** The cursor in `path`, from the focused group's editor: on every change, and when it shows a buffer. */
export function noteCursor(path: string, state: EditorState, edited: boolean): void {
  const head = state.selection.main.head;
  const line = state.doc.lineAt(head);
  report?.({ path, mode: "edit", line: line.number, column: head - line.from }, edited);
}

type Target = Pick<Workspace, "project" | "openPath" | "mode" | "reveal" | "openFile">;

/** Shows `place` in the focused group, reopening its tab if it was closed, with the cursor where it was. */
function show(workspace: Target, place: Place): void {
  // A view covering the files makes way, as for a file picked in the palette.
  mainView.close(false);
  const { path, mode, line, column = 0 } = place;
  if (mode === "edit" && line !== undefined) workspace.openFile(path, "edit", { line, from: column, to: column });
  else workspace.openFile(path, mode);
}

/** A mouse's back and forward buttons. */
const BUTTONS: Record<number, "back" | "forward"> = { 3: "back", 4: "forward" };

/** Records the places the window shows and registers Go back and Go forward; returns a function that stops. */
export function registerNavigation(workspace: Target, target: Window = window): () => void {
  const histories = new Map<ProjectId, NavigationHistory>();
  /** The open project's history; null on the start screen. */
  let active: NavigationHistory | null = null;
  const visit = (place: Place, edited: boolean) => active?.visit(place, edited);
  report = visit;
  const stop = $effect.root(() => {
    $effect(() => {
      const project = workspace.project?.id;
      const path = workspace.openPath;
      const mode = workspace.mode;
      // A spot asked for in the file (a search result, a definition): the place is there.
      const spot = workspace.reveal;
      untrack(() => {
        active = project ? (histories.get(project) ?? new NavigationHistory()) : null;
        if (project && active) histories.set(project, active);
        if (!path) return;
        if (spot?.path === path) active?.arrive({ path, mode, line: spot.line, column: spot.from });
        else active?.visit({ path, mode });
      });
    });
  });
  const go = (direction: "back" | "forward") => () => {
    const place = direction === "back" ? active?.back() : active?.forward();
    if (place) show(workspace, place);
  };
  const back = go("back");
  const forward = go("forward");
  // The buttons also mean the browser's Back and Forward in a browser tab: never that.
  const onButton = (event: MouseEvent): void => {
    const direction = BUTTONS[event.button];
    if (!direction) return;
    event.preventDefault();
    if (event.type === "mouseup" && !target.document.querySelector("[aria-modal='true']"))
      (direction === "back" ? back : forward)();
  };
  target.addEventListener("mousedown", onButton);
  target.addEventListener("mouseup", onButton);
  const unregister = [
    registerCommand({
      id: "editor.go_back",
      title: "Go back",
      keys: BACK_KEYS,
      keywords: ["navigate", "previous location", "history"],
      run: back,
    }),
    registerCommand({
      id: "editor.go_forward",
      title: "Go forward",
      keys: FORWARD_KEYS,
      keywords: ["navigate", "next location", "history"],
      run: forward,
    }),
  ];
  return () => {
    stop();
    if (report === visit) report = null;
    target.removeEventListener("mousedown", onButton);
    target.removeEventListener("mouseup", onButton);
    for (const off of unregister) off();
  };
}
