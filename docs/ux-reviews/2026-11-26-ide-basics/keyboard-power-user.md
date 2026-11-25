# Oleh, the keyboard power user: IDE basics

Build: `f514c7c` on `main`, headless, scripted fake agent, Chromium from
Playwright 1.63.0, 1280x800. Linux, so Mod is Ctrl; in a browser tab Go to
file is Alt+P, Close tab Alt+W, Reopen closed tab Alt+Shift+T and the editor
groups Ctrl+Alt+1/2.
Scope: the P0 and landed P1 items of `docs/specs/ide-basics.md`, keyboard
only. That covers Shift+F10 and Menu key menus on tree rows, tabs and the
editor; F2, Delete, Ctrl+X/C/V/Z in the tree; Shift+Arrow multi-select;
Find, Replace, Go to line, Replace in files, Go to symbol, Format, Ctrl+/;
split editor; Move tab left/right; Reopen closed tab; Save all; recent files
in Go to file; Settings > Keyboard; non-Latin layouts; palette coverage.
Fixtures: `small` (seeded).
Screenshots: `/mnt/project-files/ovra/screenshots/ide-basics/kb-*.png`.

## Summary

Oleh can do almost every everyday file and editor move without the mouse.
Shift+F10 and the Menu key open a focused menu on tree rows, tabs and the
editor, Escape always puts focus back on the row or the text, and every
shortcut in scope works on a Ukrainian layout. The worst problems are both
in the editor. Ctrl+Shift+S pressed while typing saves only the current file
and leaves the others unsaved. Global shortcuts also keep running behind the
modal "Save changes?" dialog: a Go to file opened under it gets stuck open
with focus on the page body, and Escape and Ctrl+Shift+P stop working.
Smaller problems: a renamed file's old name stays in the tree as a deleted
row that F2 and the menu still offer to rename. Tree Cut, Copy, Paste and
Comment line have no palette command. Escape does not leave the Replace in
files fields. Overall the new features feel quick and consistent; what is
left are a few traps where a key does something else than it says.

Measured: every Shift+F10 / Menu key menu was open with its first item
focused within the 150 ms check. The palette's "Keyboard shortcuts" list
and Settings > Keyboard ("Everywhere") show the same 46 commands; the pane
adds 11 groups of context keys. All 12 synthetic Ukrainian-layout presses
(each with the layout's own character and the physical `code`) ran their
action: Ctrl+X/V/Z in the tree, Ctrl+Shift+H, Ctrl+Shift+O, Ctrl+G, Ctrl+F,
Ctrl+/ (sent as `.` on the Slash key), Ctrl+\ (sent as the layout's
character on the Backslash key), Ctrl+Alt+1, Ctrl+Shift+PageUp and
Alt+Shift+T.

## Findings

### KEY-1 major: Ctrl+Shift+S (Save all) inside the editor saves only the active file

- **Area:** `ui/src/editor` (`buffers.svelte.ts`, `editorKeymap`: the `Mod-s` binding)
- **Steps:** 1. Alt+P `lib.rs` Enter, Ctrl+E, Ctrl+End, type `// x`. 2. Alt+P `Cargo.toml` Enter, Ctrl+E, Ctrl+End, type `// x`. Both tabs read "unsaved changes". 3. With the cursor still in `Cargo.toml`, press Ctrl+Shift+S. 4. Ctrl+Shift+E, press Ctrl+Shift+S again.
- **Expected:** step 3 saves both files, as the palette and Settings > Keyboard promise ("Save all Ctrl+Shift+S").
- **Actual:** step 3 sends one `fs.write_file`, for `Cargo.toml` only. Tabs afterwards: `lib.rs, unsaved changes | Cargo.toml`. The same key pressed from the tree (step 4) saves both. In another run the active file was clean, and Ctrl+Shift+S in it rewrote that file unchanged while two dirty files stayed unsaved. CodeMirror's `Mod-s` binding also matches with Shift held, runs Save file and prevents the default action, so the global Save all never runs.
- **Why it matters for Oleh:** he presses Save all from the text before running tests or handing over to an agent. It quietly does less than it says, and he finds out only when the agent or the build reads the old file.
- **Suggested fix:** let the editor keymap ignore Shift on `Mod-s` (an explicit `{ key: "Mod-Shift-s", run: () => false }` before it, or check `event.shiftKey` in `run`) so the global command handles it. Add a unit test in `keymap.test.ts` that Ctrl+Shift+S in an editor reaches `editor.save_all`.

### KEY-2 major: shortcuts run behind the "Save changes?" dialog, and Go to file opened there gets stuck

- **Area:** `ui/src/keys` (`keybindings.ts`, `installKeybindings`), `ui/src/editor` (`Tabs.svelte`, the close question), `ui/src/palette` (closing when focus is gone)
- **Steps:** 1. Open `Cargo.toml`, Ctrl+E, type a line. 2. Alt+W: the modal "Save changes to Cargo.toml before closing?" opens with focus on Save. 3. Press Alt+Shift+T, then Ctrl+\, then Alt+P, then type `readme`. 4. Press Escape. 5. Press Escape again, then Ctrl+Shift+P, then type `zzz`.
- **Expected:** while the modal question is open, only its own keys work (Save, Don't save, Cancel, Escape). Other shortcuts do nothing until it is answered.
- **Actual:** step 3: Ctrl+\ splits the editor behind the dialog (two `Cargo.toml` tabs). In the first run, Alt+Shift+T reopened a closed tab there. Alt+P opens Go to file under the dialog, where it is inert (screenshot `kb-palette-under-save-dialog.png`), and `readme` goes nowhere. Step 4: Escape cancels the dialog, focus lands on `BODY` and Go to file stays drawn on top. Step 5: Escape, Ctrl+Shift+P and typing do nothing, so there is no keyboard way out (`kb-palette-stuck-after-dialog.png`). In another run, focus went back into the editor under the still-open palette, and the next keys were typed into the file.
- **Why it matters for Oleh:** he closes tabs with Alt+W and goes straight on with the next shortcut without reading the dialog. One habit leaves him with a frozen palette and no keyboard escape, and he has to reach for the mouse or reload.
- **Suggested fix:** in `installKeybindings`, return early while a modal `dialog[open]` is in the document (or while the event target is inside one), as the context menu does with `stopPropagation`. Let the palette close itself when it loses focus to `BODY` or opens while a modal is up. Add an e2e step: Alt+W on a dirty tab, Alt+P, Escape, check the palette is closed and focus is on the tab or text.

### KEY-3 minor: after a rename or move, the old name stays in the tree as a deleted row that still offers Rename, Cut and Duplicate

- **Area:** `ui/src/tree` (`FileTree.svelte` row keys, `menu.ts` `rowMenu`), with the listing from `crates/fs-index` (`listing.rs` lists deleted tracked files)
- **Steps:** 1. In the tree, type `readme`, F2, type `NOTES.md`, Enter. 2. Wait one second. 3. Press End (focus lands on `README.md`), press Enter. 4. Press F2, type `X.md`, Enter. 5. Shift+F10 on that row.
- **Expected:** if the tree shows the old name as a git deletion, it is clearly a different kind of row: Enter shows its diff (or says "Deleted. Restore it from Git"), and F2, Cut, Copy, Duplicate and Move to Trash are left out of its menu and do nothing.
- **Actual:** right after the rename the tree is correct. About a second later, after `fs.git_changed`, a struck-through `README.md` comes back below `NOTES.md` (`kb-deleted-row-after-rename.png`). The same happens with Ctrl+X / Ctrl+V: `Cargo.toml` reappears at the root after it moved into `docs/`. Typeahead and arrows stop on it. Enter on it does nothing at all, and F2 there fails with `README.md: No such file or directory (os error 2)`. Its Shift+F10 menu is the full file menu.
- **Why it matters for Oleh:** he moves by typeahead, and after a rename `rea` can land on the ghost instead of the file he meant. Then Enter silently does nothing, and the menu offers actions that can only fail with an OS error.
- **Suggested fix:** give deleted rows their own short menu (Copy path, Show change) and handle Enter there by opening the git diff. Make F2, Delete and Ctrl+X/C skip them with a one-line note. Optionally leave deleted rows out of typeahead.

### KEY-4 minor: tree Cut, Copy, Paste and the editor's Comment line are not in the palette

- **Area:** `ui/src/tree` (`commands.ts`), `ui/src/editor` (`commands.ts`)
- **Steps:** 1. Ctrl+Shift+P and type `cut`, `paste`, `comment` and `fold` in turn.
- **Expected:** the spec's rule "Every menu item is also a command in the palette": Cut, Copy, Paste (files) and Comment line (Ctrl+/) are listed with their keys.
- **Actual:** `paste` and `comment` find nothing. `cut` finds only fuzzy noise ("Keyboard shortcuts", "Focus first editor group"…). `copy` finds only the path commands. Copy paths and Copy relative paths for a selection exist only in the menu. `fold` finds nothing, which is expected: folding is P1 and has not landed.
- **Why it matters for Oleh:** he checks the palette to learn or confirm a key. Commands that exist only as row keys or in a menu look like they do not exist.
- **Suggested fix:** register `tree.cut`, `tree.copy` and `tree.paste` (acting on the tree's selection, `keys` shown as context keys) and `editor.toggle_comment` (Ctrl+/, running CodeMirror's `toggleComment`).

### KEY-5 minor: two "Copy path" commands with different targets; the first one ignores the focused tree row

- **Area:** `ui/src/editor` (`file-commands.ts`, `file.copy_path`), `ui/src/tree` (`commands.ts`, `tree.copy_path`)
- **Steps:** 1. Open `src/main.rs`. 2. Ctrl+Shift+E, Home (row `assets`). 3. Ctrl+Shift+P, type `copy path`, Enter.
- **Expected:** with the tree focused, "Copy path" copies the focused row, or the two commands say which thing they copy.
- **Actual:** the list reads "File Copy path ; Files Copy file path ; Files Copy relative file path". Enter on the first copies `/…/demo-app/src/main.rs`, the open file, not `assets`. The categories "File" and "Files" are the only difference.
- **Why it matters for Oleh:** he pastes the path into a terminal or a task and gets the wrong one without noticing.
- **Suggested fix:** name them by target: "Copy path of open file" and "Copy path of selected row" (plus the relative variants). Or keep one "Copy path" that follows focus: the tree's row when the tree has focus, else the open file.

### KEY-6 minor: Escape does not leave the Replace in files fields

- **Area:** `ui/src/search` (`SearchPanel.svelte`, `onQueryKeydown` and the replace field)
- **Steps:** 1. From the editor, Ctrl+Shift+H (focus is in "Replace with"). 2. Press Escape. 3. Shift+Tab to the query field, press Escape.
- **Expected:** Escape returns to where he came from (the editor), as the panel's own hint "Escape: back." and every other panel do.
- **Actual:** focus stays in "Replace with", and in the query field Escape only stops a running search. To get back he needs Ctrl+Shift+E or several Tab presses.
- **Why it matters for Oleh:** "Esc closes anything" is his main way back. It works in the palette, menus, Go to line, Find, the trash question and Settings, but not here.
- **Suggested fix:** when no search is running, Escape in either field focuses the editor group that was active, or the element focused before Ctrl+Shift+F/H.

### KEY-7 minor: the trash question is not modal and stays after focus moves on

- **Area:** `ui/src/tree` (`TrashConfirm.svelte`, `actions.askTrash`)
- **Steps:** 1. Type `car` in the tree, press Delete: "Move “Cargo.toml” to Trash?" with focus on Move to Trash. 2. Shift+Tab, then Escape. 3. Alt+P `lib.rs` Enter, then Ctrl+Shift+E, ArrowDown.
- **Expected:** Escape anywhere in the Files panel cancels the question, or the question goes away once focus leaves it, as the close-tab question keeps Tab inside.
- **Actual:** Shift+Tab leaves the question; Escape on the toolbar does nothing. After opening another file and moving to `src/main.rs`, the panel still asks about `Cargo.toml` (`kb-stale-trash-question.png`). One stray Tab and Enter later would trash a file he no longer has in view.
- **Why it matters for Oleh:** he expects a question to either have his focus or be gone. A leftover destructive button is a trap, even if the trash can be restored.
- **Suggested fix:** cancel the question on focusout from it (except to its own buttons) or on Escape in the Files panel. Or keep Tab inside it as the close-tab dialog does.

### KEY-8 minor: Settings > Keyboard lists editor-only keys under "Everywhere" and leaves out the editor's menu key

- **Area:** `ui/src/settings` (`Settings.svelte`, `keyGroups`), `ui/src/editor` (`Editor.svelte` context keys, `commands.ts` LSP keys)
- **Steps:** 1. Ctrl+Shift+P, `keyboard`, Enter on "Settings Keyboard". 2. Read "Everywhere" and "Editor".
- **Expected:** keys that only work in the text are grouped under Editor. Each place that opens a menu with Shift+F10 says so. The menu's own keys (arrows, first letter, Home/End, Alt for the other variant) are listed once.
- **Actual:** "Everywhere" includes "Language: Rename symbol F2", "Go to definition F12", "Find references Shift+F12", "Format document Alt+Shift+F", "Show hover at cursor Ctrl+I" and "Go to next problem F8". F2 in the tree is Rename file and listed under Files, so the sheet shows F2 twice with two meanings. "Editor" lists only Ctrl+/ and Shift+F8; its Shift+F10 menu is not there (Files, Git and Tabs list theirs). The menu's own keys are not listed anywhere.
- **Why it matters for Oleh:** this sheet is where he learns the keys once. Wrong groups teach him that F2 renames a symbol everywhere, and the hold-Alt variants (Copy relative path) are hard to discover.
- **Suggested fix:** let a command carry a `where` (or derive it from `inEditor`) and list those under Editor. Declare `Shift+F10` for Editor, and a "Menus" group: Up/Down, Home/End, first letter, Enter, Escape, "Hold Alt: other variant".

### KEY-9 minor: tree selection does not extend with Shift+Home/End/PageUp/PageDown, and Ctrl+A does nothing

- **Area:** `ui/src/tree` (`FileTree.svelte`, `onkeydown`)
- **Steps:** 1. In the tree, Home, ArrowDown ×4, Shift+ArrowDown ×2 (three rows selected, as expected). 2. Press Shift+End. 3. Press Ctrl+A.
- **Expected:** Shift+End selects from the anchor to the last row, Shift+PageDown by a page, and Ctrl+A selects every shown row, as in VS Code's explorer and the ARIA tree pattern.
- **Actual:** Shift+End drops the selection and selects only the last row (`README.md`). Ctrl+A leaves the selection as it is.
- **Why it matters for Oleh:** selecting a run of generated files to trash or move takes one key press per row.
- **Suggested fix:** use `extendTo` for Home, End, PageUp and PageDown when Shift is held, and add Ctrl+A (select all shown rows). List them in the Files context keys.

### KEY-10 minor: Ctrl+H, the Replace key on Linux and Windows, is not bound; in a browser tab it opens History

- **Area:** `ui/src/editor` (`commands.ts`, `SEARCH_ACTIONS`)
- **Steps:** 1. Open `lib.rs`, Ctrl+E. 2. Press Ctrl+H.
- **Expected:** Replace in file opens, as in VS Code on Linux and Windows. Ctrl+Alt+F is VS Code's macOS key.
- **Actual:** nothing happens in the editor. In a real browser tab the browser's History page opens, because Ovra does not take the key. Replace is Ctrl+Alt+F.
- **Why it matters for Oleh:** muscle memory sends him out of the app.
- **Suggested fix:** use `Ctrl+H` for Replace in file on non-Apple keyboards (keep Cmd+Alt+F on Apple), and list it in the editor menu and the Keyboard pane.

### KEY-11 polish: Ctrl+Z after Move to Trash says "Nothing to undo"

- **Area:** `ui/src/tree` (`actions.svelte.ts`, `step`)
- **Steps:** 1. Delete on a row, Enter. The note says "Moved to Trash. Restore it from the Trash." 2. Ctrl+Z.
- **Expected:** a note that says why: "Move to Trash cannot be undone here; restore it from the Trash". Or the trash is undone, which the P1 row "Undo the last file operation" suggests.
- **Actual:** "Nothing to undo", although the last file operation was the trash.
- **Why it matters for Oleh:** he reads it as "the undo stack lost my operation".
- **Suggested fix:** record trash in the history as a step that only explains itself on undo, until `fs.restore` exists.

### KEY-12 polish: the Go to line field has no label

- **Area:** `ui/src/editor` (the CodeMirror `gotoLine` panel)
- **Steps:** 1. In the editor, Ctrl+G.
- **Expected:** focus on a field named "Go to line", like the Find field ("Find") and the Replace field ("Replace").
- **Actual:** focus is on an unnamed `input` (the visible text "Go to line:" is not linked to it).
- **Why it matters for Oleh:** minor. He sees the panel, but the focus check his scripts use (and screen readers) shows a bare input.
- **Suggested fix:** set `aria-label="Go to line"` on the input when the panel opens, or replace the panel with the palette's line mode.

Not findings, as planned: code folding and format on save are P1 and not
landed (Ctrl+Shift+[ does nothing; "fold" finds no command). Ctrl+1/2 do
nothing in a browser tab by design; Ctrl+Alt+1/2 work and are shown.

## What worked well

- Shift+F10 and the Menu key open the right menu on tree rows, a multi-row
  selection (Cut, Copy, Copy paths, Move to Trash…), tabs and the editor.
  Focus goes to the first item, End and a first letter move, Escape and Tab
  close and give focus back to the same row, tab or cursor
  (`kb-tree-row-menu.png`, `kb-tab-menu.png`, `kb-editor-menu.png`).
- F2 renames in place with the stem selected (`README` of `README.md`). Bad
  names keep focus in the field with an inline reason ("No / in a name",
  "“data” exists"). Escape cancels back to the row, and a renamed open file's
  tab follows it.
- Ctrl+X then Ctrl+V on a folder moves, Ctrl+C then Ctrl+V duplicates as
  `Cargo copy.toml`, and Ctrl+Z / Ctrl+Shift+Z undo and redo a rename. All of
  them work by physical key on a Ukrainian layout.
- Delete asks with focus on Move to Trash; Escape cancels to the same row, and
  after trashing, focus moves to the next row.
- New file… from the palette takes `x/y/z.ts`, makes the folders and opens the
  file for editing.
- Ctrl+G, Ctrl+F, Ctrl+Alt+F, Ctrl+Shift+O and Ctrl+/ all work in the
  editor and return to the text on Escape. Format without a server says "No
  language server is running for this file."
- Ctrl+\ splits, Ctrl+Alt+1/2 move between groups with focus in the text, Go
  to file opens into the focused group (`kb-split.png`). Ctrl+Shift+PageUp/
  PageDown move the tab and keep focus in the text. Alt+Shift+T reopens the
  last closed tab.
- Alt+P with an empty query shows a "Recent" group first, and "Recent files"
  in the palette lists them on their own.
- Settings > Keyboard has a filter and lists every command with its live key
  plus the context keys per place (`kb-settings-keyboard.png`).
