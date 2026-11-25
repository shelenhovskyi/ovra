# Split editor

Status: proposed 2026-11-25 (P1 in `docs/specs/ide-basics.md`).

People compare a file with its test, or keep a type next to the code that
uses it. Today the main area shows one file. This spec adds a second editor
group beside the first, and nothing more.

## Behaviour

- At most two groups, side by side (no vertical split, no grids). A third
  "split" moves focus instead.
- **Split right** (Mod+\\, the tab menu, the editor menu) opens the active
  file in a new group on the right; with two groups it moves focus to the
  other one.
- Each group has its own tab strip, preview tab, pinned tabs and View or
  Edit mode. Closing a group's last tab closes the group; the other one takes
  the whole width.
- The same file may be open in both groups. Both views edit one buffer: one
  text, one undo history, one unsaved state (CodeMirror's split-view
  pattern: one `EditorState` source, transactions mirrored to the second
  view). Saving from either saves the file.
- Opening a file (tree, palette, search, agent links) opens it in the
  focused group. Dragging a tab onto the other group moves it there.
- Mod+1 and Mod+2 focus a group. The splitter between them resizes and
  remembers its ratio, as the side panels do (`ui/src/shell/layout.svelte.ts`).
- The groups and their tabs are restored on reopen, like the open files are
  today.

## Changes

1. **Workspace state** (hot spot, own commit first): `Workspace` gains
   `group: 0 | 1` (the focused group) and keeps `openPath` and `mode` as the
   focused group's file and mode, so every panel that reads them keeps
   working. A small `groups` field holds the other group's `openPath` and
   `mode`.
2. **Project state protocol** (hot spot, own commit): `projects.set_state`
   gets an optional `second_group: { open_files, active_file }`; an older
   state without it opens one group. Rust and TS change together.
3. **Editor area**: `OpenFiles` becomes one instance per group;
   `Buffers` stays one per project and hands a second, mirrored view to a
   group that shows a file the other group shows.
4. **Commands**: `editor.split_right`, `editor.focus_group_1`,
   `editor.focus_group_2`, `editor.move_tab_to_other_group`.

## Not in this spec

Diff side by side (the git diff has its own view), more than two groups,
vertical splits, and terminals in a group.

## Tests

Unit tests for the mirrored buffer (an edit in one view shows in the other;
undo works from either), group open and close rules and state restore; one
e2e that splits, edits in the right group and saves.
