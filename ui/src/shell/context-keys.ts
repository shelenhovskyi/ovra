// Keys that work in one place, for Settings > Keyboard (keys/context-keys.ts).
// The shell declares the keys of its main-area views, and, until those
// panels declare their own, the keys of the panels it lays out. Each panel
// handles its keys itself; nothing here binds them.

import type { ContextKey } from "../keys/context-keys";
import { trashKeys } from "../tree/ops";

export const SHELL_KEYS: ContextKey[] = [
  { keys: "Escape", title: "Close Settings, Reviews or the Audit log", where: "Views" },
];

/** The panels' keys, as each panel handles them today. */
export const PANEL_KEYS: ContextKey[] = [
  { keys: "Y", title: "Allow once", where: "Approvals" },
  { keys: "N", title: "Deny", where: "Approvals" },
  { keys: "Shift+N", title: "Deny and tell Claude why", where: "Approvals" },
  { keys: "Up/Down", title: "Previous or next request", where: "Approvals" },
  { keys: "Escape", title: "Leave the request waiting", where: "Approvals" },
  { keys: "Enter", title: "Start the task, or send the reply", where: "Agents" },
  { keys: "Shift+Enter", title: "New line", where: "Agents" },
  { keys: "F2", title: "Rename", where: "Files" },
  { keys: trashKeys(), title: "Move to the trash", where: "Files" },
  { keys: "Shift+F10", title: "Open the row's menu (also the Menu key)", where: "Files" },
  { keys: "S", title: "Stage", where: "Git" },
  { keys: "U", title: "Unstage", where: "Git" },
  { keys: "O", title: "Open the file", where: "Git" },
  { keys: "Enter", title: "Show the diff", where: "Git" },
  { keys: "Delete", title: "Discard changes…", where: "Git" },
  { keys: "Shift+F10", title: "Open the row's menu (also the Menu key)", where: "Git" },
  { keys: "Mod+Enter", title: "Commit", where: "Git" },
  { keys: "N", title: "Next change", where: "Diffs" },
  { keys: "P", title: "Previous change", where: "Diffs" },
  { keys: "Shift+F8", title: "Go to previous problem", where: "Editor" },
  { keys: "Shift+F10", title: "Open the tab's menu (also the Menu key)", where: "Tabs" },
  { keys: "Left/Right", title: "Previous or next tab", where: "Tabs" },
  { keys: "Delete", title: "Close the tab", where: "Tabs" },
  { keys: "Alt+Shift+P", title: "Pick an element on the page", where: "Agent browser" },
];
