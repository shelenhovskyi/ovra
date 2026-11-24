# Keyboard power user (Oleh): Round 3 (2026-11-24), regression

Build: `9512ea6` on `main`, headless, real core (debug build), Chromium 158, 1280x800, Linux (Mod = Ctrl).
Scope: re-check of every round 1 and round 2 finding (KEY-1 to KEY-21), plus what changed since `485e5f2`:
hot exit for unsaved text and the dirty-tab close prompt, the project switcher, transport
disconnect handling, browser-safe shortcuts in headless mode (Alt+W, Alt+], Alt+[, Alt+P),
Go to file over `fs.list_files`, virtualized viewers, and tree refresh for visible rows only.
Fixtures: `small`, `monorepo` (seeded). Keyboard only (`page.keyboard`), with
`document.activeElement` logged after each step and a window `keydown` spy recording
`defaultPrevented`. There were 12 scripted runs (s1-s12).

## Summary

Oleh can now do the whole session from the keyboard, and fast: Alt+P, `p42m7`, Enter opens
`packages/pkg-042/src/module-7.ts` in 7 keystrokes (it took 98 in round 2). The tree has
typeahead, paging and a visible focus ring, and the project list uses arrow keys. Unsaved
text survives a reload, a project close and Alt+W, and the confirm puts focus on Save.
Three things still cost him: focus still drops to `<body>` on four common routes (closing
the last tab, Ctrl+E back to view, Go to file from the editor, Close project). Alt+P
followed straight away by the query and Enter loses the Enter while the file list loads.
In a browser, Ctrl+P and Ctrl+S (in view mode) still reach the browser's print and
"Save page as" dialogs. Overall: 12 fixed, 6 partly fixed, 3 still open, 0 regressed.

## Regression table

| ID     | Round | Old severity | Status       | Evidence (round 3)                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------ | ----- | ------------ | ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| KEY-1  | 1     | major        | Fixed        | Alt+P opens "Go to file in monorepo". `p42m7` ranks `packages/pkg-042/src/module-7.ts` first (s2), so the trip takes 7 keystrokes, down from 98. The first list took 107 ms and later opens 15 ms (s7). Ctrl+P itself is tracked under KEY-18.                                                                                                                                                                                          |
| KEY-2  | 1     | major        | Fixed        | After Enter on a project, focus is on `div[role=tree]` (s1, s2). Switch project also lands in the tree (s2). Esc from the palette on the start screen returns focus to the project button (s1).                                                                                                                                                                                                                                         |
| KEY-3  | 1     | minor        | Fixed        | Typing `r` selects `README.md`, `src` selects `src`, and `file-04999` in the 5,005-row folder selects it and scrolls it into view (s1, s2). Names now sort naturally: `.gitignore, a file with spaces.txt, build.log, Cargo.toml`.                                                                                                                                                                                                      |
| KEY-4  | 1     | minor        | Fixed        | A focused tree draws a 1 px inset ring, `#4c6ef5` on `#16161a`, at about 4.2:1 (s1 `04-after-open.png`). The first ArrowDown selects `assets`, and `.git` is no longer listed.                                                                                                                                                                                                                                                          |
| KEY-5  | 1     | minor        | Fixed        | PageDown moves the selection `file-00000` → `00031` → `00062` and keeps it in view. PageUp goes back, and Home right after PageDown selects and shows the first row (`st=0`) (s2).                                                                                                                                                                                                                                                      |
| KEY-6  | 1     | minor        | Partly fixed | The palette has 12 commands, including Go to file, Switch project, Close project, Reveal, Collapse all and Changed only. Still open: "Show all commands" lists itself and Enter on it does nothing (s6). There is no focus-editor or focus-tree command with a key (Reveal works as a focus-tree command but has no shortcut). On the start screen it lists Close tab, Next tab and Toggle editing, none of which can do anything (s1). |
| KEY-7  | 1     | minor        | Partly fixed | Tab now goes tree → tablist (one stop) → "Open in default app" → file content, and Shift+Tab reverses it (s4). Tab from the editor or the viewer's last stop still lands on `<body>` (s7 `Tab after link: body`, s10 `Esc, Tab from editor: body`). The status bar is not a stop, and F6 does nothing (`F6:false`, s12).                                                                                                                |
| KEY-8  | 1     | minor        | Fixed        | One Tab stop for the list: ArrowDown, End and Home move between `demo-app` and `monorepo`, and Tab goes on to the folder field (s2).                                                                                                                                                                                                                                                                                                    |
| KEY-9  | 1     | polish       | Still open   | The empty main area says "Open a file from the tree on the left." with no shortcut hint (s5). The start screen and status bar show none either.                                                                                                                                                                                                                                                                                         |
| KEY-10 | 1     | polish       | Still open   | `document.title` is `Ovra` with a project and a file open (s1, s5, s7).                                                                                                                                                                                                                                                                                                                                                                 |
| KEY-11 | 2     | blocker      | Fixed        | Dirty `lib.rs`, then a reload and reopen: the tab comes back as "lib.rs, unsaved changes", the viewer says "Showing unsaved changes from the editor." with `// U1`, and Ctrl+E shows the text (s4). It also survives Close project and reopen (s10). No `beforeunload` prompt appears, because the drafts are flushed instead.                                                                                                          |
| KEY-12 | 2     | major        | Fixed        | Alt+W on a dirty tab shows "Save changes to lib.rs before closing?" with focus on Save. Enter saves and closes. Tab, Enter on "Don't save" discards and moves focus into the next tab's content. The editor bar no longer stays behind (s4, s5). What is left: KEY-22 and KEY-25.                                                                                                                                                       |
| KEY-13 | 2     | major        | Fixed        | In a browser tab, close and next/previous tab are on Alt+W, Alt+] and Alt+[, and the palette shows those keys (s1). All three work from the tree, the viewer and CodeMirror (s4, s12). Ctrl+W is no longer captured (`C-w:false`), so the browser may close the tab, but the drafts survive that (KEY-11).                                                                                                                              |
| KEY-14 | 2     | major        | Partly fixed | Ctrl+E puts the caret in CodeMirror (`AE Ctrl+E: div[role=textbox].cm-content`, s4). Ctrl+E back to view still drops focus to `<body>` (s4, s12; see KEY-22). There is still no command with a key that focuses the editor from the tree.                                                                                                                                                                                               |
| KEY-15 | 2     | major        | Partly fixed | Fixed: Alt+]/Alt+[ in edit mode (focus goes into the new tab's editor), Alt+] from the viewer content (focus stays in the content), closing a tab with others left, and Enter on a markdown link (focus goes to the README content, and the tree follows) (s4, s7, s12). Still `<body>`: Go to file from the editor, Ctrl+E to view, and closing the last tab (KEY-22).                                                                 |
| KEY-16 | 2     | major        | Fixed        | Type `// U1`, Alt+], Alt+[, then Ctrl+Z removes `// U1`, and Ctrl+Shift+Z brings it back (s4).                                                                                                                                                                                                                                                                                                                                          |
| KEY-17 | 2     | minor        | Fixed        | The tablist is one Tab stop, ArrowRight and End move between tabs, the close buttons have `tabindex=-1`, and the tabs have `aria-controls` (s4).                                                                                                                                                                                                                                                                                        |
| KEY-18 | 2     | minor        | Still open   | In a browser tab, Ctrl+P is not prevented (`C-p:false`, s1, s5), because `Mod+P` was swapped for Alt+P, so the browser's print dialog opens. Ctrl+S with a file in view mode is also not prevented (`C-s:false`, s4, s5), so "Save page as" opens.                                                                                                                                                                                      |
| KEY-19 | 2     | minor        | Partly fixed | View mode now shows the unsaved buffer with "Showing unsaved changes from the editor." (s4). Still open: Enter in the tree (s5: Cargo.toml; s12: the revealed row) and Go to file (s4, s12) always open in view mode, even while editing. After a reload, the restored active file opens in view mode (s5).                                                                                                                             |
| KEY-20 | 2     | minor        | Partly fixed | With one tab, reopening after a reload selects `module-7.ts` and shows it (`inview=true st=500`, s8). With two tabs (`module-7.ts` active, `module-33.ts` second), reopening after a reload shows no selected row and `scrollTop=0`, even after 2 s. Switching to another project and back does the same (s9, s7). The Reveal command fixes it every time.                                                                              |
| KEY-21 | 2     | polish       | Fixed        | Ctrl+E on `data/blob.bin` or `assets/logo.png` stays in the viewer and shows "… is a binary file and cannot be edited here." (s5).                                                                                                                                                                                                                                                                                                      |

Counts: 12 fixed (KEY-1, 2, 3, 4, 5, 8, 11, 12, 13, 16, 17, 21), 6 partly fixed (KEY-6, 7,
14, 15, 19, 20), 3 still open (KEY-9, 10, 18), 0 regressed.

## New findings

### KEY-22 major: Focus still drops to `<body>` after four common actions

- **Area:** `ui/src/editor` (close the last tab, Ctrl+E back to view), `ui/src/palette` (focus restore after Go to file), `ui/src/shell` (Close project and the default focus target).
- **Steps:**
  1. Open `demo-app`. Alt+P, `README.md`, Enter, then Ctrl+E (the caret is in the editor). Press Alt+W. This is the only tab (s6).
  2. Open `lib.rs`, press Ctrl+E (caret in editor), then press Ctrl+E again to go back to view (s4, s12).
  3. In the editor of `main.rs`, press Alt+P, type `strings.rs`, then press Enter (s12).
  4. With a project open, press Ctrl+Shift+P, type `close project`, then press Enter (s2, s10).
- **Expected:** Focus goes to the next sensible target: the tree after closing the last tab, the viewer content after leaving edit mode, the new file's content after Go to file, and the first project in the list after Close project.
- **Actual:** In all four cases `document.activeElement` is `<body>` (`Alt+W last tab from editor: body`, `Ctrl+E edit->view: body`, `finder from edit: body`, `after close project: body`). The next ArrowDown does nothing. After Close project, the first Tab still stays on `<body>` and only the second reaches `demo-app` (s10). In case 3 the palette tries to restore focus to the CodeMirror element, which the switch to view mode has just removed.
- **Why it matters for Oleh:** these are his most frequent moves: close, flip to view, jump to a file. Each one ends with a key that silently does nothing, then blind Tabs.
- **Suggested fix:** Add one shell-level fallback. When focus would land on `<body>` while the main area changes (the `openPath` or `mode` change, or the last tab closing), focus the new content region, or the tree when no file is open. The palette should check `previous.isConnected` after the command has run, not before, and fall back to the same target. Close project should focus the first project button.

### KEY-23 major: Go to file drops Enter while the file list is loading

- **Area:** `ui/src/palette` (`Palette.svelte` `run` with `items` empty while `entries === null`)
- **Steps (s9, s8):** 1. Seed `monorepo`, open it. 2. Without pausing, press Alt+P, type `p42m7`, and press Enter, the way he types `:e` in Vim.
- **Expected:** The Enter applies to the best match as soon as the list arrives, or the list is ready first.
- **Actual:** The list still says "Loading…" when Enter is pressed. Enter does nothing, and 800 ms later the palette is still open with no file opened and no tab (`after 800ms palette open: 1 tabs (none)`). Pressing Enter again works. With a warm list the same keys work (`warm fast: tabs module-7.ts*`). After a reload, the first Alt+P is always cold.
- **Why it matters for Oleh:** typing ahead is his whole speed model. A dropped Enter means he looks up, sees the palette still open, and types it again, every first jump of a session.
- **Suggested fix:** If Enter arrives while `entries === null`, remember it and run the top item once the entries load (still applying the query). Optionally prefetch `fs.list_files` when the project opens, so the first Alt+P is warm.

### KEY-24 minor: The close confirm answers Esc only while focus is inside it, and Tab walks out of it

- **Area:** `ui/src/editor` (`Tabs.svelte`, the `role="alertdialog"` bar)
- **Steps (s5):** 1. Make `lib.rs` dirty in the editor, then press Alt+W. Focus is on Save. 2. Press Tab 4 times. 3. Press Esc. 4. Type a character.
- **Expected:** Either a real modal (Tab cycles Save / Don't save / Cancel, Esc cancels from anywhere) or a non-modal bar that still answers Esc wherever focus is, and does not accept typing into the file being closed.
- **Actual:** Tab goes Save → Don't save → Cancel → the editor bar's **Save** → the CodeMirror text (`Tab 4: div[role=textbox].cm-content`). Esc there leaves the question open (`dialog count after Esc: 1`), and typed characters go into the buffer under the question. Esc works only once focus is back on a dialog button.
- **Why it matters for Oleh:** Esc is his universal "back out". A question that ignores Esc as soon as he Tabs once feels broken.
- **Suggested fix:** Keep Tab inside the three buttons (wrap) while `tabs.closing` is set, and handle Esc on the window while it is open. Return focus to where it was on Cancel (that already works from a button: focus goes back to the tab).

### KEY-25 minor: Closing the last tab, or Enter=Save on the last dirty tab, leaves nothing focused

This is the last-tab case of KEY-22. It is listed separately because it also follows the
confirm.

- **Area:** `ui/src/editor` (`Tabs.svelte` `answer`, which calls `focusTab(null)` when there is no tab left)
- **Steps (s5):** 1. Have one dirty tab. 2. Press Alt+W, then Enter (Save).
- **Expected:** The file saves and closes, and focus goes to the tree (with the file still selected).
- **Actual:** It saves and closes, and focus is on `<body>` (`Enter=Save: body`, no tabs left).
- **Suggested fix:** When `focusTab` finds no tab, focus the tree viewport.

### KEY-26 polish: Go to file says "100 results" when more files match

- **Area:** `ui/src/palette` (`sources.ts` `limit: 100`, the status announcement)
- **Steps (s7):** In `monorepo`, press Alt+P and type `module-7.ts` (200 files match).
- **Expected:** "100 of 200 results", or a hint to type more.
- **Actual:** The list stops at `pkg-099`, and the status says "100 results". ArrowUp from the top wraps to `pkg-099`, which looks like the end of the list.
- **Suggested fix:** Show "showing 100 of N" when the cap applies.

### KEY-27 polish: The palette offers commands that cannot run where it is opened

- **Area:** `ui/src/palette` (commands source), `ui/src/editor` and `ui/src/keys` (registration)
- **Steps (s1, s6):** 1. On the start screen, press Ctrl+Shift+P. 2. With a project open but no tab, do the same.
- **Expected:** Only commands that do something now, as VS Code does with `when` clauses. At least "Show all commands" should not list itself.
- **Actual:** On the start screen the list shows Close project, Close tab, Next tab, Previous tab and Toggle editing. With no tab, it also shows Save file Ctrl+S. Running them does nothing and says nothing. "Show all commands" lists itself, and Enter on it leaves the palette open with nothing changed.
- **Suggested fix:** Add an optional `enabled()` to `Command` (this is a `ui/src/keys/commands.ts` hot-spot change) and filter on it. Drop `palette.show` from its own list.

### KEY-28 polish: A Tab pressed right after "Connected" can land in the folder field

- **Area:** `ui/src/tree` (`ProjectList.svelte`)
- **Steps:** Load the page and press Tab, then Enter, as soon as the status says "Connected".
- **Expected:** Tab reaches the first project.
- **Actual:** In 1 of about 12 runs (the first s4 attempt), the folder field had focus with `demo-app` already listed, and Enter in the empty field did nothing (s4 `failure.png`). In the other runs the project button appeared 33 ms after "Connected". This is a race, seen once.
- **Suggested fix:** On load, focus the first project once the list arrives (or the folder field when the list is empty), so the first key never depends on timing.

## What worked well

- Go to file is the jump Oleh asked for: the fuzzy ranking puts the right file first for `p42m7`, `pkg042module7`, `042 module-7` and `pkg-042/module-7`. Arrows wrap, each reopen clears the query, and after Enter the tree reveals the file without taking focus.
- Tree navigation is now complete: typeahead (also inside 5,005 rows, with 41 DOM rows), PageUp/PageDown, Home/End, a visible ring on focus, and a selection that stays put when a file is added outside (`README.md` stays selected after `AAA-new.txt` appears, s6).
- Edit mode is properly keyboard-first: Ctrl+E puts the caret in the editor, Alt+]/Alt+[ keep the caret in the next editor, and undo history survives tab switches.
- Unsaved text is safe on every path he tried: reload, Close project and reopen, and Alt+W with the Save / Don't save / Cancel confirm. The viewer says when it shows unsaved text.
- The project switcher (Ctrl+Shift+P, `swi`, Enter, `demo`, Enter) lands in the tree. On the start screen, Alt+P opens the project list directly.
- Disconnects are handled: after the core is killed, the status reads "Connection to core lost, reconnecting…", Ctrl+S shows a `role="alert"` "connection to core closed", and the caret stays in the editor (s11).
- The headless shortcuts are labelled where he looks for them: the palette shows Alt+W, Alt+], Alt+[ and Alt+P, and the original keys still apply in the desktop window.
