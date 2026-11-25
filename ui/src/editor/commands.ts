// Commands owned by the editor that work in any mode.

import { toggleComment } from "@codemirror/commands";
import { registerCommand } from "../keys/commands";
import { showList } from "../palette/mode.svelte";
import type { Workspace } from "../shell/workspace.svelte";
import { foldCommands } from "./folding";
import { editorReady, goToChange, readBlame, showChangeAtCursor, toggleBlame } from "./git/extension";
import type { LspAction } from "./lsp/actions";
import { modeMemory } from "./mode-memory";
import {
  closeOtherTabs,
  closeSavedTabs,
  closeTabsToTheRight,
  focusGroup,
  focusMain,
  moveTab,
  reopenClosedTab,
  requestClose,
  saveAll,
  show,
  splitRight,
  tabs,
} from "./open-files.svelte";
import { editorPrefs } from "./prefs.svelte";
import { REPLACE_KEYS, runSearch, type SearchAction, shownView } from "./search";
import { symbolSource } from "./symbols";

type Target = Pick<
  Workspace,
  "openPath" | "mode" | "openFile" | "paletteOpen" | "other" | "group" | "focusGroup" | "split" | "closeGroup"
>;

/** Lists the shown file's symbols in the palette; Enter jumps to one. */
async function goToSymbol(workspace: Target): Promise<void> {
  const view = shownView();
  const path = workspace.openPath;
  if (!view || !path) return;
  const list = await symbolSource(view, path);
  if (workspace.openPath === path && shownView() === view) showList(list, workspace);
}

/**
 * Switches between viewing and editing; files that cannot be edited say why
 * instead. Files of the same kind open in the chosen mode from then on.
 */
export async function toggle(workspace: Target): Promise<void> {
  const path = workspace.openPath;
  if (!path) return;
  if (workspace.mode === "edit") {
    workspace.mode = "view";
    modeMemory.set(path, "view");
    await focusMain();
    return;
  }
  tabs.notice = null;
  const buffers = tabs.current?.buffers;
  const buffer = buffers?.open(path);
  await buffer?.loaded;
  if (workspace.openPath !== path) return;
  if (buffers && buffer && buffer.status !== "ready") {
    buffers.discard(path);
    tabs.notice =
      buffer.status === "binary"
        ? `${path} is a binary file and cannot be edited here.`
        : buffer.status === "too_large"
          ? `${path} is too large to edit here.`
          : `${path} cannot be edited: ${buffer.error ?? "it could not be read"}.`;
    return;
  }
  workspace.mode = "edit";
  modeMemory.set(path, "edit");
}

/**
 * `action`, run in the editor: in view mode the open file switches to editing
 * first (the markers live there), and the action waits for its changes. A
 * file that cannot be edited says why instead, as Mod+E does.
 */
function inEditor(workspace: Target, action: () => unknown): () => Promise<void> {
  // A function, so the check after `toggle` is not narrowed away.
  const editing = () => workspace.mode === "edit";
  return async () => {
    if (!editing()) {
      const ready = editorReady();
      await toggle(workspace);
      if (!editing()) return;
      await ready;
    }
    await action();
  };
}

/** Saves the open file if it has unsaved text (loaded or as a draft); otherwise does nothing. */
async function saveActive(workspace: Target): Promise<void> {
  const path = workspace.openPath;
  const buffers = tabs.current?.buffers;
  if (!path || !buffers?.isDirty(path)) return;
  await buffers.open(path).save();
}

/** The language-server actions, with the keys the editor binds for them (shown in the palette). */
const LSP_ACTIONS: [LspAction, string, string?][] = [
  ["definition", "Go to definition", "F12"],
  ["references", "Find references", "Shift+F12"],
  ["rename", "Rename symbol", "F2"],
  ["format", "Format document", "Shift+Alt+F"],
  ["hover", "Show hover at cursor", "Mod+I"],
  ["problems", "Show problems", "Mod+Shift+M"],
  ["next_problem", "Go to next problem", "F8"],
];

/**
 * CodeMirror's panels, with the keys VS Code uses. Inside the editor its own
 * keymap answers Mod+F; Ctrl+G and Mod+Alt+F are bound there too (buffers.svelte.ts).
 */
const SEARCH_ACTIONS: [SearchAction, string, string][] = [
  ["find", "Find in file", "Mod+F"],
  ["replace", "Replace in file", REPLACE_KEYS],
  ["go_to_line", "Go to line…", "Ctrl+G"],
];

const FOLD_ACTIONS: [keyof typeof foldCommands, string][] = [
  ["fold", "Fold at cursor"],
  ["unfold", "Unfold at cursor"],
  ["fold_all", "Fold all"],
  ["unfold_all", "Unfold all"],
];

/** Registers the editor and tab commands; returns a function that unregisters them. */
export function registerEditorCommands(workspace: Target): () => void {
  const step = (offset: number) => () => {
    const next = tabs.current?.neighbour(offset);
    if (next) show(workspace, next);
  };
  const move = (offset: -1 | 1) => () => {
    const active = tabs.current?.active;
    if (active) tabs.current?.shift(active, offset);
  };
  const nextChange = inEditor(workspace, () => goToChange(1));
  const previousChange = inEditor(workspace, () => goToChange(-1));
  const readCursorBlame = inEditor(workspace, readBlame);
  const showChange = inEditor(workspace, showChangeAtCursor);
  const unregister = [
    registerCommand({
      id: "editor.toggle",
      title: "Toggle editing the open file",
      keys: "Mod+E",
      run: () => toggle(workspace),
    }),
    // Registered in every mode, so Mod+S never reaches the browser's "Save page".
    registerCommand({ id: "editor.save", title: "Save file", keys: "Mod+S", run: () => saveActive(workspace) }),
    // Mod+Alt+S shows Servers; Mod+Shift+S is the closest free key (Ovra has no Save as).
    registerCommand({ id: "editor.save_all", title: "Save all", keys: "Mod+Shift+S", run: saveAll }),
    registerCommand({
      id: "editor.reopen_closed_tab",
      title: "Reopen closed tab",
      keys: "Mod+Shift+T",
      run: () => reopenClosedTab(workspace),
    }),
    // Mod+0 is the sidebar's, so resetting has no default key.
    registerCommand({
      id: "editor.zoom_in",
      title: "Zoom in file text",
      keys: "Mod+=",
      keywords: ["font", "size", "larger"],
      run: () => editorPrefs.zoom(1),
    }),
    registerCommand({
      id: "editor.zoom_out",
      title: "Zoom out file text",
      keys: "Mod+-",
      keywords: ["font", "size", "smaller"],
      run: () => editorPrefs.zoom(-1),
    }),
    registerCommand({
      id: "editor.zoom_reset",
      title: "Reset file text size",
      keywords: ["font", "zoom"],
      run: () => editorPrefs.zoom(0),
    }),
    registerCommand({
      id: "editor.toggle_word_wrap",
      title: "Toggle word wrap",
      keys: "Alt+Z",
      keywords: ["wrap", "lines"],
      run: () => {
        editorPrefs.wordWrap = !editorPrefs.wordWrap;
      },
    }),
    registerCommand({ id: "editor.next_tab", title: "Next tab", keys: "Ctrl+Tab", run: step(1) }),
    registerCommand({ id: "editor.previous_tab", title: "Previous tab", keys: "Ctrl+Shift+Tab", run: step(-1) }),
    // As in VS Code; nothing else in Ovra or CodeMirror binds them.
    registerCommand({ id: "editor.move_tab_left", title: "Move tab left", keys: "Mod+Shift+PageUp", run: move(-1) }),
    registerCommand({ id: "editor.move_tab_right", title: "Move tab right", keys: "Mod+Shift+PageDown", run: move(1) }),
    // Alt+F5 and Alt+Shift+F5 as in VS Code; neither browsers nor CodeMirror use them.
    registerCommand({ id: "editor.next_git_change", title: "Git: go to next change", keys: "Alt+F5", run: nextChange }),
    registerCommand({
      id: "editor.previous_git_change",
      title: "Git: go to previous change",
      keys: "Alt+Shift+F5",
      run: previousChange,
    }),
    // No default shortcuts: free ones that browsers leave alone are scarce; the palette reaches these.
    registerCommand({ id: "editor.toggle_blame", title: "Git: toggle blame", run: toggleBlame }),
    registerCommand({ id: "editor.read_blame", title: "Git: read blame of cursor line", run: readCursorBlame }),
    registerCommand({ id: "editor.show_git_change", title: "Git: show change at cursor", run: showChange }),
    registerCommand({
      id: "editor.close_tab",
      title: "Close tab",
      keys: "Mod+W",
      run: () => {
        const active = tabs.current?.active;
        if (active) requestClose(workspace, active);
      },
    }),
    registerCommand({
      id: "editor.close_other_tabs",
      title: "Close other tabs",
      run: () => closeOtherTabs(workspace, tabs.current?.active ?? null),
    }),
    registerCommand({
      id: "editor.close_tabs_to_the_right",
      title: "Close tabs to the right",
      run: () => {
        const active = tabs.current?.active;
        if (active) closeTabsToTheRight(workspace, active);
      },
    }),
    registerCommand({ id: "editor.close_saved_tabs", title: "Close saved tabs", run: () => closeSavedTabs(workspace) }),
    registerCommand({
      id: "editor.close_all_tabs",
      title: "Close all tabs",
      run: () => closeOtherTabs(workspace, null),
    }),
    // No default shortcut: Enter on the previewed file in the tree also keeps it.
    registerCommand({
      id: "editor.keep_tab",
      title: "Keep tab",
      keywords: ["preview"],
      run: () => {
        const active = tabs.current?.active;
        if (active) tabs.current?.makePermanent(active);
      },
    }),
    registerCommand({
      id: "editor.toggle_pin",
      title: "Pin or unpin tab",
      run: () => {
        const active = tabs.current?.active;
        if (active) tabs.current?.togglePin(active);
      },
    }),
    // Mod+\ as in VS Code. Mod+1 and Mod+2 are free in Ovra; a browser tab keeps them, so headless mode uses Mod+Alt.
    registerCommand({
      id: "editor.split_right",
      title: "Split editor right",
      keys: "Mod+\\",
      keywords: ["group", "side by side"],
      run: () => splitRight(workspace),
    }),
    registerCommand({
      id: "editor.focus_group_1",
      title: "Focus first editor group",
      keys: "Mod+1",
      run: () => focusGroup(workspace, 0),
    }),
    registerCommand({
      id: "editor.focus_group_2",
      title: "Focus second editor group",
      keys: "Mod+2",
      run: () => focusGroup(workspace, 1),
    }),
    registerCommand({
      id: "editor.move_tab_to_other_group",
      title: "Move tab to other editor group",
      keywords: ["split"],
      run: () => {
        const active = tabs.current?.active;
        if (active) moveTab(workspace, active, workspace.group === 0 ? 1 : 0);
      },
    }),
    ...LSP_ACTIONS.map(([action, title, keys]) =>
      registerCommand({
        id: `editor.lsp_${action}`,
        title: `Language: ${title}`,
        keys,
        where: "Editor",
        run: inEditor(workspace, () => tabs.current?.lsp?.action(action)),
      }),
    ),
    registerCommand({
      id: "editor.go_to_symbol",
      title: "Go to symbol in file…",
      keys: "Mod+Shift+O",
      keywords: ["outline"],
      run: inEditor(workspace, () => goToSymbol(workspace)),
    }),
    // Fold and unfold have their keys in the editor (folding.ts); fold all has none.
    ...FOLD_ACTIONS.map(([action, title]) =>
      registerCommand({
        id: `editor.${action}`,
        title,
        keywords: ["collapse", "expand"],
        run: inEditor(workspace, () => {
          const view = shownView();
          if (view) foldCommands[action](view);
        }),
      }),
    ),
    // In the text the editor's keymap answers Mod+/ itself; this is the palette's way in.
    registerCommand({
      id: "editor.toggle_comment",
      title: "Comment or uncomment lines",
      keys: "Mod+/",
      where: "Editor",
      run: inEditor(workspace, () => {
        const view = shownView();
        if (view) toggleComment(view);
      }),
    }),
    ...SEARCH_ACTIONS.map(([action, title, keys]) =>
      registerCommand({ id: `editor.${action}`, title, keys, run: inEditor(workspace, () => runSearch(action)) }),
    ),
  ];
  return () => {
    for (const off of unregister) off();
  };
}
