# Nadia, the newcomer: IDE basics (2026-11-26)

Build: f514c7c (main), headless (`just build-headless`), Playwright Chromium 158.0.8127.36, 1280x800.
Scope: the basic IDE features from `docs/specs/ide-basics.md`. File tree context menus (on a file, a folder and the empty space), New file and New folder with an inline name, Rename (F2), Duplicate, Move to Trash with its confirmation, drag and drop, multi-select with Cut, Copy, Paste and Undo, Open in terminal, Find in folder. In the editor: the right-click menu, Find, Replace, Go to line, Replace in files (Mod+Shift+H), split editor (Mod+\), the tab menu and dragging tabs. Also Settings > Editor (auto save, tab size), the status bar facts and recent files in the file finder.
Fixtures: `small` (seeded as demo-app). The harness makes a fresh temp copy for every run, so trashing fixture files was safe.

Runs: six scripts. Every file operation was checked twice: on disk, and in the `fs.*` frames in `session.log`. Screenshots are saved as `/mnt/project-files/ovra/screenshots/ide-basics/newcomer-*.png`.

## Summary

Nadia could do every everyday file job with the mouse, as she would in VS Code, and without help. Right-click a row, New file, a nested name like `helpers/format.rs`, Rename, Duplicate, drag onto a folder, Copy and Paste, and Find in folder all did what she expected. Open tabs followed renames and moves. The worst problem is Undo: Ctrl+Z right after Move to Trash does not bring the file back. Instead it silently moves a different, earlier file to the trash (NEW-1). Smaller problems:

- The editor's Find, Replace and Go to line bars are unstyled CodeMirror defaults at 9.8 px (NEW-5).
- Replace in files reports success with "Check the spelling" (NEW-8).
- The status bar says "Spaces: 4" while Tab inserts 2 (NEW-9).
- Split view squeezes both headers until they wrap (NEW-11).

Overall the menus feel familiar and calm. The rough edges are in feedback after an action, not in finding the action.

New: 0 blocker, 1 major, 10 minor, 5 polish.

Not a finding: in the browser, Ctrl+P does nothing, because the browser keeps it. The app shows "Find a file Alt+P" on the empty screen instead. The Alt+P finder lists recent files under a "Recent" heading, which works well (`newcomer-recent-files.png`).

## Findings

### NEW-1 major: Ctrl+Z after Move to Trash trashes a different, earlier file instead of restoring the one just trashed

- **Area:** ui/src/tree (`actions.svelte.ts`: `askTrash`, `undo`, the history)
- **Steps:** 1. Right-click `src/lib.rs` > Duplicate. `lib copy.rs` appears. Press Escape to keep the name. 2. Right-click `changes.patch` > Move to Trash… > Move to Trash. 3. Click the tree and press Ctrl+Z, as in VS Code, to get `changes.patch` back.
- **Expected:** `changes.patch` comes back. If a trash cannot be undone, Ctrl+Z does nothing and the tree says "Restore it from the Trash", and it never reaches past the trash to an older step.
- **Actual:** `changes.patch` stays in the trash. The core receives `fs.trash {"paths":["src/lib copy.rs"]}`, and the duplicate from step 1 disappears from the tree. Nothing on screen says that anything was undone (`newcomer-after-trash.png`, then `newcomer-undo-after-trash.png`: `lib copy.rs` is gone).
- **Why it matters for Nadia:** Ctrl+Z is her safety net. Here it removes a second file she did not point at, quietly. If she had already edited the copy, she would not know where her work went.
- **Suggested fix:** Record the trash in the undo history. The easiest version is a "barrier" entry: Ctrl+Z then stops there and says "Moved to Trash can't be undone here; restore it from the Trash". The better version restores the file, if the trash service can. Always show a short notice for what Undo did ("Undid Duplicate: lib copy.rs moved to Trash").

### NEW-2 minor: The trash question appears at the top of the tree, away from the file, and says nothing about unsaved changes or folder contents

- **Area:** ui/src/tree (`TrashConfirm.svelte`, where FileTree places it)
- **Steps:** 1. Scroll the tree so `changes.patch` is near the bottom. 2. Right-click it > Move to Trash…. 3. Separately: open `a file with spaces.txt`, click Edit, type a line without saving, then trash the file the same way. 4. Press Delete on the folder `ui`.
- **Expected:** The question sits next to the row it is about, or at least keeps that row in view. It warns "has unsaved changes" and, for a folder, says how much is inside ("ui and 1 file").
- **Actual:** The card is inserted above the first row. It pushes the whole list down by about 80 px, and the row being trashed scrolls out of view (`newcomer-trash-confirm.png`). The text is always just `Move "<name>" to Trash?`, also for a file with unsaved edits and for a folder. After the dirty file is trashed, its tab stays open with "a file with spaces.txt was deleted on disk. Save as a new file / Close without saving". That keeps her text, but "deleted on disk" is not how she would describe what she just did herself (`newcomer-trash-dirty.png`).
- **Why it matters for Nadia:** She reads the question literally. She should see what she is about to lose next to the thing she clicked.
- **Suggested fix:** Show the card as a popover anchored to the row, or keep the row scrolled into view. Add "Unsaved changes will be lost" when an open tab is dirty, and "and N items" for folders. Change the tab notice after the person's own trash to "Moved to Trash. Save as a new file / Close".

### NEW-3 minor: "Moved to Trash. Restore it from the Trash." never goes away and stacks with the next question

- **Area:** ui/src/tree (FileTree notice)
- **Steps:** 1. Trash `changes.patch`. 2. Open and close a few files, rename one. 3. Press Delete on `ui`.
- **Expected:** A short notice that fades out or can be dismissed. Ideally it has an Undo or Restore button.
- **Actual:** The two-line notice stays above the tree through every later step. When the next trash question opens, both cards are stacked and push the tree down by about 150 px (`newcomer-undo-after-trash.png`).
- **Why it matters for Nadia:** Old messages that never leave look like a problem still waiting for her.
- **Suggested fix:** Let it time out (a few seconds), clear it on the next file operation, or give it a close ×. If NEW-1 adds restore, put an "Undo" button in it.

### NEW-4 minor: After Rename, Move or Move to Trash, the old name stays in the tree, struck through

- **Area:** ui/src/tree (git decorations on rows)
- **Steps:** 1. Rename `README.md` to `readme.md`, or drag `README.md` into `docs`, or trash `a file with spaces.txt`.
- **Expected:** The old name disappears, as in Finder and VS Code's tree. The change shows in Git, or a deleted row is clearly labelled.
- **Actual:** The old name stays in place, struck through with a red dot, next to the new one (`newcomer-duplicate-rename.png`: `README.md` struck through above `readme.md`; `newcomer-trash-dirty.png`). Only screen readers hear "deleted". The folder shows "contains deleted files".
- **Why it matters for Nadia:** She just trashed a file and it is still in the list. First she thinks the trash failed, then she thinks the rename made a copy.
- **Suggested fix:** Show git-deleted rows only with the "Changed only" filter, or move them into the Git panel. If they stay, add a visible "deleted" label and a tooltip "Deleted, still in git. Commit or restore in Git".

### NEW-5 minor: Find, Replace and Go to line are unstyled CodeMirror bars with 9.8 px text

- **Area:** ui/src/editor (search panel theme, `search.ts`)
- **Steps:** 1. Open `src/lib.rs`. 2. Right-click > Find (or Ctrl+F). 3. Right-click > Go to line (or Ctrl+G, or click "Ln 1, Col 1" in the status bar).
- **Expected:** A compact find bar in the app's Quiet style: the same Aa, ab and .* toggles as the Search panel, sentence-case labels, text at 12 px or more.
- **Actual:** A bar at the bottom of the editor with lowercase "next", "previous", "all", "replace", "replace all", native checkboxes "match case", "regexp", "by word", and inputs and buttons at 9.8 px (the app's body is 14 px) (`newcomer-find-bar.png`). Go to line is "Go to line:" with a tiny field and a "go" button (`newcomer-go-to-line.png`). A line number past the end goes to the last line without a word.
- **Why it matters for Nadia:** This is the one place that looks like a different, unfinished app. 9.8 px is hard to read, and "by word" or "regexp" do not match the Search panel she just learned.
- **Suggested fix:** Theme the CodeMirror panels with the app's tokens (12 to 13 px, same button style), reuse the Search panel's toggle icons with tooltips, and use "Go to line" with placeholder "Line number" and Enter to go.

### NEW-6 minor: Ctrl+H, the Replace shortcut she knows, does nothing

- **Area:** ui/src/keys (editor bindings), ui/src/editor
- **Steps:** 1. In Edit mode in `src/lib.rs`, press Ctrl+H.
- **Expected:** Replace opens, as in VS Code (Ctrl+H), or a hint says "Replace is Ctrl+Alt+F".
- **Actual:** Nothing happens and focus stays in the text. Replace is on Ctrl+Alt+F (menu and palette), which she would never guess.
- **Why it matters for Nadia:** She finds Find right away, then thinks Replace is missing.
- **Suggested fix:** In the desktop app, also bind Mod+H to Replace in file (it is free there). In the browser, where Ctrl+H is history, show Ctrl+Alt+F in the find bar's Replace toggle tooltip.

### NEW-7 minor: Ctrl+F in View mode silently switches the file to Edit mode, and the View-mode menu has no Find

- **Area:** ui/src/viewers (`Viewer.svelte` menu), ui/src/editor
- **Steps:** 1. Click `src/lib.rs` (it opens in View mode). 2. Right-click the text. 3. Press Ctrl+F.
- **Expected:** The menu offers Find and Go to line, because both work here. Searching does not change the mode.
- **Actual:** The View-mode menu has only "Select all", "Copy path", "Copy relative path" and "Reveal in file tree" (`newcomer-view-menu.png`). Ctrl+F works, but the header flips to "View Ctrl+E / Save" (Edit mode), and the language-server notice appears (`newcomer-find-bar.png`).
- **Why it matters for Nadia:** She only wanted to find a word, and now the file is editable without her asking. In Edit mode, the same right-click shows nine more items. The two menus for the same text feel random.
- **Suggested fix:** Add Find, Go to line and Go to symbol to the viewer menu. Keep the mode when searching, or say "Switched to Edit to search".

### NEW-8 minor: Replace in files says "Check the spelling" after a successful Replace all, and leaves open files unsaved without saying so

- **Area:** ui/src/search (`SearchPanel.svelte`, `replace.ts`)
- **Steps:** 1. Open `src/lib.rs` in Edit mode. 2. Press Ctrl+Shift+H. 3. Type `a + b` and `a.wrapping_add(b)`, then click Replace all > Replace.
- **Expected:** The cursor starts in the search field. Afterwards the panel says something like "Replaced 2 matches in 2 files. 1 open file has unsaved changes. Save all (Ctrl+Shift+S)".
- **Actual:** Ctrl+Shift+H puts the cursor in "Replace" while "Search in files" is still empty, so her first typing goes into the wrong field (`newcomer-replace-in-files-focus.png`). After the replace, the panel shows "Replaced 2 matches in 2 files." and right below it a "No results for "a + b"" empty state that says "Check the spelling." (`newcomer-replace-in-files-done.png`). `changes.patch` was written to disk. `src/lib.rs` was only changed in its open tab (unsaved dot), which the message does not mention.
- **Why it matters for Nadia:** "Check the spelling" reads as an error right after it worked. Not knowing that one file still needs saving means a half-done refactor on disk.
- **Suggested fix:** Focus the search field when it is empty. After a replace, show only the result line, without the no-results empty state. Name the files left unsaved and offer Save all.

### NEW-9 minor: The status bar says "Spaces: 4" while Tab inserts 2 spaces

- **Area:** ui/src/editor (`status.svelte.ts`, `indent.ts`)
- **Steps:** 1. Settings > Editor > Tab size 2. 2. Open `src/lib.rs` (indented with 4) in Edit mode. 3. At the end, press Enter, Tab and type `x`.
- **Expected:** The status bar shows what Tab will insert, or Tab follows the file's detected indentation that the status bar shows.
- **Actual:** The status bar says "Spaces: 4". Tab inserts 2 spaces (`  x`, Ln 5, Col 4) (`newcomer-tab-size-mismatch.png`).
- **Why it matters for Nadia:** She trusts the status bar to tell her how the file is indented. Mixed indentation then shows up in review.
- **Suggested fix:** Pick one rule. Either use the detected indentation for editing when the file has one (as VS Code's "detect indentation" does), or show the setting. Make the fact clickable to switch, which is P2 in the spec.

### NEW-10 minor: Creating, renaming or deleting a file needs a right-click; the Files header and the palette give no hint

- **Area:** ui/src/tree (`FileTree.svelte` header, `commands.ts` palette keywords)
- **Steps:** 1. Open demo-app and look for a way to make a new file without right-clicking. 2. In the palette (Ctrl+Shift+P), type "delete".
- **Expected:** The Files header has New file and New folder buttons, as in VS Code's Explorer. Typing "delete" finds "Move file to Trash".
- **Actual:** The header has only Collapse all, Reveal and Changed only (`newcomer-file-menu.png`). The empty screen lists New task, Find a file, Search and Terminal, but not New file. The palette search for "delete" returns nothing. "rename", "replace" and "go to line" do find their commands.
- **Why it matters for Nadia:** She uses the mouse first and looks at headers before she thinks of right-clicking. "Delete" is the word she uses for removing a file.
- **Suggested fix:** Add two quiet icon buttons (New file, New folder) to the Files header, shown on hover like the others. Add the keywords "delete" and "remove" to `tree.trash`, and "create" and "add" to `tree.new_file`.

### NEW-11 minor: Split editor squeezes both headers until they wrap and clips the first group's tabs

- **Area:** ui/src/shell (`EditorGroup.svelte`), ui/src/editor (file header)
- **Steps:** 1. Open three files. 2. Press Ctrl+\ (or tab right-click > Split right) at 1280x800 with both side panels open.
- **Expected:** Two readable editors. The header shortens gracefully (icon-only View or Edit, the file name ellipsised at the start).
- **Actual:** Each group is about 315 px wide. The path shows "Cargo.t…", "View" and "Ctrl+E" wrap onto two lines in both groups, and the first group's third tab is cut off ("main.rs" clipped, with no overflow hint) (`newcomer-split.png`).
- **Why it matters for Nadia:** Her first split looks broken, so she will close it again.
- **Suggested fix:** Use `white-space: nowrap` on header buttons, drop the key cap below a width threshold, ellipsise the path, and add a fade or scroll hint on a clipped tab strip.

### NEW-12 polish: The tab menu is long and starts with paths, not Close

- **Area:** ui/src/editor (`Tabs.svelte` menu)
- **Steps:** 1. Right-click a tab.
- **Expected:** Close, Close others, Close to the right, Close saved and Close all first, as in VS Code. At most about 10 items (`docs/design/components.md`).
- **Actual:** 12 items in 3 groups. Copy path, Copy relative path, Reveal in file tree and Open Containing Folder come first, and the Close group is last. "Close" shows "Alt+W" in the second group's menu but no shortcut in the first group's (`newcomer-tab-menu.png`).
- **Suggested fix:** Put the Close group first and the path group last. Show the close shortcut consistently.

### NEW-13 polish: Menu words are inconsistent: "Open Containing Folder", "Reveal project folder", the hidden "Copy relative path"

- **Area:** ui/src/tree (`ops.ts` `revealLabel`, `menu.ts`)
- **Steps:** 1. Right-click a file, then the empty space, then a tab.
- **Expected:** One sentence-case wording everywhere (`docs/specs/quiet-style.md`), and the same path items in every menu.
- **Actual:** File and folder rows say "Open Containing Folder" (Title Case, the only one). The empty space says "Reveal project folder". The editor and tab menus say "Reveal in file tree". The tree menu shows "Copy relative path" only while Alt is held, but the editor and tab menus always list it.
- **Suggested fix:** Use "Open containing folder" (or "Show in file manager") everywhere, including the root ("Open project folder"). List "Copy relative path" in the tree menu too, or drop the Alt trick.

### NEW-14 polish: A red "Could not copy" appears after files were cut or copied correctly

- **Area:** ui/src/tree (`actions.svelte.ts` `copyPaths` called from cut and copy)
- **Steps:** 1. In the browser without clipboard permission, select `Cargo.toml` and `.gitignore` and press Ctrl+C. 2. Paste into `docs`. 3. Select `build.log` and press Ctrl+X.
- **Expected:** The file copy works and nothing complains. If the paths could not go to the system clipboard, nothing is said, because that is a bonus.
- **Actual:** The paste works (`fs.copy` twice), but the tree header shows a red "Could not copy" (`newcomer-could-not-copy.png`).
- **Suggested fix:** Ignore clipboard errors when the clipboard write is only a side effect of Cut or Copy. Keep the error for the explicit "Copy path".

### NEW-15 polish: Find in folder fills "src/**", but the empty state still says "Searches every file except ignored ones."

- **Area:** ui/src/search
- **Steps:** 1. Right-click `src` > Find in folder….
- **Expected:** "Searches src" (or "Searches files in src/**") until she types.
- **Actual:** The scope is set correctly in "Files to include", and the results stay inside `src`. The hint under it still says "Searches every file except ignored ones." (`newcomer-find-in-folder.png`).
- **Suggested fix:** Build the hint from the include and exclude fields.

### NEW-16 polish: "Auto save: After a delay" does not say how long

- **Area:** ui/src/settings (`EditorPane.svelte`)
- **Steps:** 1. Settings > Editor > Auto save > After a delay. 2. Type in a file.
- **Expected:** "After 1 second" (or a delay field).
- **Actual:** It saves after about 1 s (measured 1010 ms), but the option only says "After a delay" (`newcomer-settings-editor.png`).
- **Suggested fix:** Name the delay in the option label.

## What worked well

- Right-click on a file, a folder and the empty space gives short, familiar menus in VS Code's order, with shortcuts on the right and Move to Trash last in red (`newcomer-file-menu.png`, `newcomer-root-menu.png`). Shift+F10 and the Menu key open the same menu with focus on the first item.
- New file asks for the name in place. It explains bad names in plain words ("No / at the start or end", "Enter a name", "Not a valid name", "“README.md” exists"), and `helpers/format.rs` creates the folder and opens the file.
- F2 selects only "Cargo" in "Cargo.toml". Duplicate opens "lib copy.rs" with "lib copy" selected for renaming (`newcomer-duplicate-rename.png`). Copy and Paste into the same folder makes "Cargo copy.toml" instead of failing.
- Drag and drop highlights the target folder (`newcomer-drop-target.png`). Undo moves the file back, and the open tab follows renames and moves, unsaved edits included.
- Open in terminal starts a shell in the right folder. Find in folder scopes the search to `src` and puts the cursor in the search box.
- The editor menu adapts: Cut and Copy only with a selection, language items only when a server serves the file, Split right while there is one group (`newcomer-editor-menu.png`). The status bar facts (Ln and Col, selection count, language, indentation, line endings) are quiet and useful, and clicking the cursor position opens Go to line.
- Replace in files asks "Replace 2 matches in 2 files?" before writing.
- The file finder shows "Recent" above "Files". Settings > Editor is short and clear, and auto save works.
