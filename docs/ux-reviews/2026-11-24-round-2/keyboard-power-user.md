# Keyboard power user (Oleh): Round 2 (2026-11-24)

Build: `f427218` on `main` (485e5f2 plus the fixture additions), headless, real core, Chromium 158.0.8127.36, 1280x800, Linux (Mod = Ctrl).
Scope: viewers (`ui/src/viewers`), the editor with tabs (`ui/src/editor`: Tabs, commands, view/edit toggle, save, CodeMirror keys against global shortcuts, switching and closing tabs by keyboard), projects (opening, switching, per-project state after reload), and the commands now in the palette. Fixtures: `small`, `monorepo`. Keyboard only (`page.keyboard`); `document.activeElement` was logged after every step. There were 10 scripted runs (s0-s9 in the scratch folder).

## Summary

Oleh can now open, view, edit and save a file and cycle tabs without the
mouse, and the pieces are quick: viewers render in 17-32 ms, Mod+S saves with
a conflict check, and tabs and expanded folders come back per project. But
**unsaved text is silently lost on reload**, confirmed below with exact steps,
and Mod+W closes a modified tab without asking. In a browser, Mod+W and
Ctrl+Tab belong to the browser, not to Ovra, so the same keys can close the
whole app. Focus is still the weak spot: Mod+E does not put the caret in the
editor, and switching or closing tabs drops focus to `<body>`, so every edit
costs 3 to 2n+1 extra Tab presses.

## Findings

### KEY-11 blocker: Reloading (or closing the window) silently discards unsaved edits

- **Area:** `ui/src/editor` (`session.svelte.ts`: unsaved text lives only in the module-level `unsaved` map), `ui/src/shell` (no unload guard).
- **Steps (s3, reproduced every run):**
  1. Seed `small`, load the app. Press Tab, then Enter to open `demo-app`, then Tab into the tree.
  2. Press ArrowDown 6 times (to `src`), ArrowRight, then ArrowDown 2 times (to `src/lib.rs`), then Enter.
  3. Press Ctrl+E, then Tab 3 times to reach the editor, then Ctrl+End, and type `// UNSAVED-1 UNSAVED-2`. The tab shows `●`, and the bar shows `●`.
  4. Reload the page (F5 / Ctrl+R; the script used `page.reload()`).
  5. Tab, then Enter on `demo-app`, then Ctrl+E.
- **Expected:** Before the unload, a "You have unsaved changes" prompt (`beforeunload`). If the page is reloaded anyway, the text comes back (hot exit), or at least a notice says it was discarded.
- **Actual:** No dialog fires (`page.on("dialog")` logged nothing). After the reload the start screen shows, the tab comes back clean, and the editor holds the disk text: `"pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n"`. Both `UNSAVED-1` and `UNSAVED-2` are gone, and nothing says so. `session.log` shows no `fs.write_file` and no state write for the buffer (s3 `04-after-reload.png`, `05-after-reload-editor.png`). `ui/src` has no `beforeunload` handler; only the tree's `pagehide` flush exists.
- **Why it matters for Oleh:** he reloads out of habit (Ctrl+R, or after a tool hiccup). Closing the browser tab with Ctrl+W (KEY-13) takes the same path. Losing work silently is the worst outcome an editor can have.
- **Suggested fix:** Short term: add a `beforeunload` listener in the editor that calls `preventDefault()` while `dirtyFiles` is non-empty. In Tauri, add a close-requested guard (app glue, asking the UI) that does the same. Proper fix: hot exit, which keeps unsaved buffers with the project state and restores them with a "restored unsaved changes" marker. That needs a protocol field, so it is a small protocol task first.

### KEY-12 major: Mod+W closes a modified tab without asking, and the edits become invisible

- **Area:** `ui/src/editor` (`commands.ts` `editor.close_tab`, `Tabs.svelte` `close`, `Editor.svelte`)
- **Steps (s3):** 1. Do steps 1-3 of KEY-11 (`lib.rs` modified, caret in the editor). 2. Press Ctrl+W.
- **Expected:** A confirm with Save / Don't save / Cancel (keyboard: Enter = Save, Esc = Cancel), with focus on its default button. Afterwards, focus goes to the next tab's editor or to the tree.
- **Actual:** The tab closes at once, with no prompt. `projects.set_state` writes `open_files: []`. Focus drops to `<body>`. The main area keeps a stale editor bar showing `●`, `Rust` and an enabled **Save** button, but no path (s3 `02-after-close.png`). Pressing Mod+S or the Save button there does nothing, because the session's view is already destroyed. The text survives only in memory: reopening `lib.rs` from the tree shows the _disk_ version in the viewer, while the tab shows `●`. Ctrl+E then brings back `// UNSAVED-1`. A reload in between loses it (KEY-11). The same happens when the close button is activated with Enter.
- **Why it matters for Oleh:** Mod+W is muscle memory: he closes tabs in bursts. Every other editor he uses asks first.
- **Suggested fix:** In `editor.close_tab` and the close button, if `hasUnsavedChanges(project, path)`, show a small keyboard-first dialog (Save / Don't save / Cancel). "Don't save" also clears the stash and the dirty flag. Reset `session` to null when `openPath` becomes null, so the ghost bar goes away. After closing, focus the editor of the new active tab, or the tree when no tabs are left.

### KEY-13 major: Ctrl+W, Ctrl+Tab and Ctrl+Shift+Tab are browser-reserved, so in headless (browser) mode they act on the browser, not on Ovra

- **Area:** `ui/src/editor` (`commands.ts` bindings), `ui/src/keys`
- **Steps:** 1. Run `just dev-headless` and open the UI in Chrome. 2. Open a file, make an edit, and press Ctrl+W (or Ctrl+Tab).
- **Expected:** Ovra's shortcuts never collide with keys the browser keeps for itself (the persona file lists exactly Ctrl+W, T, L and N). Where they must, Ovra offers a browser-safe alternative.
- **Actual:** Chromium does not deliver Ctrl+W, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+T or Ctrl+N to pages, and they cannot be prevented: Ctrl+W closes the whole Ovra browser tab, and Ctrl+Tab switches browser tabs. Combined with KEY-11, Ctrl+W in headless mode is one keystroke to lose all unsaved work. Playwright cannot show this, because its CDP key events bypass browser accelerators: in the harness all three keys reached Ovra (`C-w:true`, `C-Tab:true`). So this is from the shortcut table plus known Chromium behaviour, not a screenshot. The palette shows these as _the_ shortcuts ("Close tab Ctrl+W", "Next tab Ctrl+Tab"). In Tauri they work.
- **Why it matters for Oleh:** headless mode is how he runs Ovra on a remote box. The advertised shortcut for "close tab" there means "close the IDE".
- **Suggested fix:** Keep Mod+W and Ctrl+Tab in Tauri, but give the headless build browser-safe defaults and show those in the palette. Candidates: Alt+W to close the tab, and Alt+PageDown / Alt+PageUp or Ctrl+Alt+Right / Left to switch tabs. The choice is a UX decision for the owner. KEY-11's `beforeunload` guard is the safety net either way.

### KEY-14 major: Mod+E does not focus the editor; reaching the text takes 2n+1 Tabs, and Tab inside it indents

- **Area:** `ui/src/editor` (`Editor.svelte`, `commands.ts`), `ui/src/shell` (focus targets)
- **Steps (s2, s4):** 1. Open `src/lib.rs` from the tree, so focus is in the tree. 2. Press Ctrl+E. 3. Start typing.
- **Expected:** Ctrl+E switches to edit mode _and_ puts the caret in the editor (VS Code and Vim both do this). Esc, or a "focus tree" command, gets back out.
- **Actual:** Focus stays on the tree (`AE Mod+E: div[role=tree]`), so typed characters go to the tree, and a `\n` there re-opens the file in view mode. To reach the text he must Tab past every tab and its close button: 3 Tabs with one tab open, **7 with three** (s4 `Tabs to reach editor: 7`). Inside the editor, Tab indents (`indentWithTab`) and Shift+Tab dedents. The way out is Esc and then Tab, which is CodeMirror's convention, but nothing says so, and that Tab lands on `<body>`; reaching the tree takes one more Tab. Toggling back with Ctrl+E from the editor leaves focus on `<body>`.
- **Why it matters for Oleh:** every edit starts with a mode switch. Six blind Tab presses before he can type breaks his flow every time.
- **Suggested fix:** After Ctrl+E switches to edit, focus the CodeMirror view once it is mounted (`view.focus()` after `load()`). When it switches back to view, focus the viewer or the tree. Register `tree.focus` and `editor.focus` commands, for example Ctrl+0 and Ctrl+1, or F6 to cycle panels (see KEY-7).

### KEY-15 major: Switching tabs, closing a tab or following a markdown link drops focus to `<body>`

- **Area:** `ui/src/editor` (tab commands), `ui/src/viewers` (MarkdownView)
- **Steps:** 1. (s4) With the caret in the editor of `lib.rs` and three tabs open, press Ctrl+Tab. 2. (s4) Press Ctrl+W on a saved tab. 3. (s6) Open `docs/guide.md`, Tab 3 times to the `README` link, and press Enter.
- **Expected:** Focus follows the content: the editor of the new tab in edit mode, or the main area or tree in view mode.
- **Actual:** `document.activeElement` is `<body>` after each step (`Ctrl+Tab from cm (dirty): body`, `Ctrl+W in cm (saved): body`, `Enter on link: body`). The next Tab after the link went nowhere (`Tab after link: body`). The tree selection also stays on the old file after Ctrl+Tab and after the link, so the tree and the tabs disagree.
- **Why it matters for Oleh:** every tab switch costs another round of blind Tab presses (KEY-14). He cannot chain Ctrl+Tab, type, and Ctrl+Tab.
- **Suggested fix:** Before the active editor or viewer unmounts, remember whether focus was inside the main area, and if so restore it into the new content. Sync the tree selection to `openPath` when it changes from outside the tree.

### KEY-16 major: Switching tabs or toggling view/edit throws away the undo history

- **Area:** `ui/src/editor` (`session.svelte.ts`: `destroy()` stashes only the text, and `load()` builds a fresh `EditorState`)
- **Steps (s4):** 1. In the editor of `lib.rs`, type `XYZ`. 2. Press Ctrl+Tab, then Ctrl+Shift+Tab back. 3. Tab into the editor, then press Ctrl+Z.
- **Expected:** `XYZ` is undone.
- **Actual:** Nothing changes: the text still ends in `XYZ`. The tab is still marked modified, but undo has nothing left. The same happens after Ctrl+E to view and Ctrl+E back (s2). The cursor position and scroll are also reset.
- **Why it matters for Oleh:** he flips to another file to check something and then wants to undo. Undo that silently does nothing is a trap, especially with the round trip through view mode that Ctrl+E encourages.
- **Suggested fix:** Keep each open tab's `EditorState` (with history and selection) alive while the tab is open, rather than its text only. Create a new view from the kept state on return, or keep hidden views.

### KEY-17 minor: The tab strip is 2 Tab stops per tab and ignores arrow keys

- **Area:** `ui/src/editor` (`Tabs.svelte`)
- **Steps (s4, s5):** 1. Open six files. 2. Tab from the tree. 3. With focus on a `role="tab"`, press ArrowRight.
- **Expected:** One Tab stop for the whole tablist, with ArrowLeft and ArrowRight moving between tabs (the WAI-ARIA tabs pattern the markup already claims), and Delete closing the focused tab.
- **Actual:** Each tab and each close button is a Tab stop, so six tabs make 12 stops before the content. ArrowRight does nothing (`ArrowRight in tablist: button[role=tab]|lib.rs`). The tablist has no `aria-controls` or `tabpanel`.
- **Why it matters for Oleh:** together with KEY-14, the tab strip becomes a wall between the tree and the editor.
- **Suggested fix:** Use a roving `tabindex` on the tab buttons with Left, Right, Home and End. Take the close buttons out of the Tab order (`tabindex="-1"`) and make Delete close the focused tab.

### KEY-18 minor: Ctrl+S in view mode and Ctrl+P still fall through to the browser

- **Area:** `ui/src/editor` (`Editor.svelte` registers `editor.save` only while mounted), `ui/src/palette` / `ui/src/keys` (KEY-1)
- **Steps (s4):** 1. With a file open in view mode, focus on the tree, press Ctrl+S. 2. Press Ctrl+P.
- **Expected:** Ovra swallows both, even when it has nothing to do with them.
- **Actual:** The window listener logs `C-s:false` and `C-p:false` (not prevented). A browser opens "Save page as" and the print dialog. In edit mode Ctrl+S is fine (`C-s:true`).
- **Suggested fix:** Register `editor.save` globally (as a no-op in view mode, or one that saves the active modified tab), and bind Ctrl+P (see KEY-1).

### KEY-19 minor: The mode follows the route, not the user: the viewer shows disk text for a modified tab, and opening from the tree leaves edit mode

- **Area:** `ui/src/viewers`, `ui/src/editor`, `ui/src/tree` (the `onopen` call)
- **Steps (s2, s3):** 1. Edit `lib.rs` without saving, then press Ctrl+E to switch to view mode. 2. In edit mode, press Enter on a file in the tree. 3. Reload with a file open in edit mode.
- **Expected:** View mode either renders the unsaved buffer or says "Showing the saved version; unsaved changes in editor". The mode is kept when he opens another file.
- **Actual:** 1. The viewer shows the saved text while the tab shows `●` (s2 `04-view-after-edit.png`). 2. Enter in the tree always switches to view mode (`openFile(path)` defaults to `"view"`), while Ctrl+Tab keeps edit mode. 3. After a reload, the restored file opens in view mode.
- **Suggested fix:** Pass the current mode from the tree's `onopen`, or let the user pick a default. Add a one-line notice in the viewer when the file has unsaved changes.

### KEY-20 minor: After a reload, the tree does not reveal the restored file

- **Area:** `ui/src/tree`
- **Steps (s8):** 1. In `monorepo`, open `packages/pkg-042/src/module-7.ts` (98 keystrokes by arrows). 2. Reload, then Tab and Enter on `monorepo`.
- **Expected:** The tree is scrolled so that `module-7.ts` is visible and highlighted.
- **Actual:** The expanded folders and the tab come back, but `scrollTop` is 0 (it was 1372), and no highlighted row is in view. Focus is on `<body>` (KEY-2). The first ArrowDown in the tree jumps off-screen to `module-8.ts`.
- **Suggested fix:** After restoring, scroll `openPath` into view, or add the "Reveal open file" command from round 1.

### KEY-21 polish: Edit mode is offered for binary, image and too-large files

- **Area:** `ui/src/editor`
- **Steps (s5):** Open `data/blob.bin`, `assets/logo.png` or `data/huge.txt`, then press Ctrl+E.
- **Actual:** The editor bar shows "Plain text" and a Save button over "This is a binary file and cannot be edited here." (the PNG too) or "too large to edit". The messages are clear, but it is one wasted key, plus another to get back.
- **Suggested fix:** Make Ctrl+E a no-op with a short status hint when the viewer already knows the file is binary or too large.

## Round 1 re-check

| Round 1                                                   | Severity | Status                     | Evidence (round 2)                                                                                                                                                                                                                             |
| --------------------------------------------------------- | -------- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| KEY-1 No Mod+P file finder; Ctrl+P falls through to print | major    | **Still open**             | Ctrl+P is not prevented (`C-p:false`). Reaching `packages/pkg-042/src/module-7.ts` by arrows took 98 keystrokes (103 in round 1). The palette finds nothing for `file` except "Toggle editing the open file".                                  |
| KEY-2 Focus lost after opening or adding a project        | major    | **Still open**             | Tab, then Enter on a project: `after open project: body`. ArrowDown right after does nothing. The same happens after a reload restores tabs (s8).                                                                                              |
| KEY-3 No typeahead; byte-order sort                       | minor    | Still open                 | Typing `r` in the tree does nothing. `README.md` still sorts before `a file with spaces.txt`.                                                                                                                                                  |
| KEY-4 No focus ring on a tree with nothing selected       | minor    | Still open                 | `outline=none`, no selected row after Tab into the tree (s7 `01-tree-focus.png`).                                                                                                                                                              |
| KEY-5 PageUp/PageDown do not move the selection           | minor    | Still open                 | There is no PageDown case in `FileTree.svelte` `onkeydown`.                                                                                                                                                                                    |
| KEY-6 Palette has nothing to run                          | minor    | **Changed (partly fixed)** | Now 5 commands, 6 in edit mode: Close tab, Next/Previous tab, Save file, Toggle editing, Show command palette, each with its shortcut. Still missing: switch/close project, focus tree/editor, reveal file. `palette.show` still lists itself. |
| KEY-7 Tab dead-ends; main area unreachable                | minor    | **Changed**                | Tab from the tree now goes tab, close, (editor), `<body>`, then tree. Viewer content is reachable only through links, there is no F6, and Shift+Tab from the tree still leaves the app.                                                        |
| KEY-8 Project list ignores arrows                         | minor    | Still open                 | `ArrowDown in project list: button                                                                                                                                                                                                             | demo-app` (unchanged). |
| KEY-9 No hint for Ctrl+Shift+P                            | polish   | Still open                 | Empty main area says "Viewer: open a file from the tree."                                                                                                                                                                                      |
| KEY-10 Window title lacks the project                     | polish   | Still open                 | `document.title` is `Ovra` with a project and a file open.                                                                                                                                                                                     |

Round 1's issue 8 ("a reload forgets the open project") is **changed**:
per-project state (tabs, active file, expanded folders) now survives a
reload, but the app still reloads to the start screen. With no
"Switch project" or "Close project" command, the only way back to the project
list is still a reload.

## What worked well

- Mod+S in the editor saves with `expected_mtime_ms`. When the file was changed on disk, it shows a `role="alert"` with Overwrite and Reload from disk, both reachable with Shift+Tab (s9). The `●` on the tab and the close button's label "Close lib.rs (unsaved changes)" are clear.
- CodeMirror and the global shortcuts coexist: Ctrl+Shift+P opens the palette from inside the editor, and Esc returns the caret to it. Ctrl+F opens CodeMirror search, and Esc returns to the text. Mod+S is handled once (`C-s:true`), never twice. Typing `save` in the palette finds "Save file Ctrl+S".
- Unsaved text survives tab switches and view/edit toggles within one page load (`cm after reopen: "...// UNSAVED-1"`). Only a reload or closing the window loses it.
- Ctrl+Tab and Ctrl+Shift+Tab wrap correctly and keep edit mode (in Tauri, and in the harness).
- Viewers are fast and clear by keyboard: markdown, image (`1 × 1`), patch with line numbers, "Binary file (application/octet-stream) cannot be shown." and "This file is too large to show." all render in 17-32 ms after Enter. Enter on a markdown link opens the target as a new tab, and `<script>` in markdown is shown as text.
- Per-project state is restored: after a reload and reopen, `monorepo` comes back with `packages`, `pkg-042` and `src` expanded and `module-7.ts` as the active tab.
