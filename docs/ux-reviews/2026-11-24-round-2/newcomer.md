# Nadia, the newcomer: Round 2 (2026-11-24)

Build: 485e5f2 (main) plus the fixture additions (f427218), headless, real core, Playwright's bundled Chromium 158.0.8127.36 (Linux), 1280x800.
Scope: viewers (markdown, image, patch, text, binary, too large), the editor with tabs (view/edit switch, saving, conflicts), projects (start screen, add/open/switch/remove, per-project state on reload), open in external editor and links. Fixtures: `small` (with a few extra files written by the scripts: `docs/links.md`, `notes/n01..n21.txt`), `empty`.

## Summary

Nadia can now do her core job: add her project, click files and read them. Markdown, the diff, images and the "binary" and "too large" cases all render with plain words, which is a big step from round 1. But as soon as she edits, Ovra loses her text without a word in four different ways: reloading the page, closing a tab and then reloading, opening a 21st file, and an agent touching a file she left in another tab. None of these shows a warning. Editing is also hard to find for a mouse user, and there is still no way back to the project list. Overall reading feels solid and calm, but editing feels unsafe, and for an app whose agents write files all the time, that is the part she has to trust.

## Findings

### NEW-11 blocker: Reloading the page throws away unsaved text silently, and afterwards nothing shows it was ever there

- **Area:** ui/src/editor (session.svelte.ts keeps unsaved text only in a JS `Map`), ui/src/shell (no `beforeunload`)
- **Steps:** 1. Start with `small` seeded and click `demo-app`. 2. Click `README.md` in the tree, press Ctrl+E, click at the end of the text and type "TODO from Nadia: an hour of notes.". 3. Click `Cargo.toml` in the tree, press Ctrl+E, type `edition = "2021"`. Both tabs now show ● (`04-before-reload.png`). 4. Press F5 (or Ctrl+R, or close the browser tab). 5. Click `demo-app` on the start screen. 6. Click the `README.md` tab and press Ctrl+E.
- **Expected:** at step 4 the browser asks "Leave site? Changes you made may not be saved". Better still, the unsaved text is kept (in the project state or local storage) and comes back with a ● on its tab. If it can't be kept, a notice says that 2 files had unsaved changes that were lost.
- **Actual:** no dialog. A synthetic `beforeunload` is not cancelled (`defaultPrevented: false`, no handler). The page reloads in about 0.7 s to the start screen (`04-after-reload.png`). After reopening the project, both tabs come back (`README.md`, `Cargo.toml`) but with no ● and the original text. "Nadia" is not in the editor or on disk, and `edition` is not in `Cargo.toml` (`04-readme-after-reload.png`). The tabs coming back makes this worse: everything _looks_ restored, so she will assume her text was saved.
- **Why it matters for Nadia:** pressing F5 when something looks stuck is a web developer's reflex. She reads the restored tabs literally as "my work is here" and only finds out much later.
- **Suggested fix:** (a) right away, register a `beforeunload` handler in the shell that calls `preventDefault()` while any file is dirty (`dirtyFiles` is already a reactive set). (b) Then persist unsaved buffers per project, next to `open_files`, and restore them with the ● on reload. Saving buffers into project state may need a small protocol/store task first.

### NEW-12 blocker: Closing a tab with unsaved changes asks nothing, keeps the text only until the next reload, and leaves a broken editor bar

- **Area:** ui/src/editor (Tabs.svelte `close`, commands.ts `editor.close_tab`, Editor.svelte)
- **Steps:** 1. Open `README.md`, press Ctrl+E, type a line. The tab shows ● and its close button is labelled "Close README.md (unsaved changes)". 2. Hover the ●: it turns into × (`03-hover-close.png`). Click it. 3. Look at the main area. 4. Click Save, or press Ctrl+S. 5. Click `README.md` in the tree again, then press Ctrl+E. 6. Type more and press Ctrl+W.
- **Expected:** step 2 asks "Save changes to README.md? Save / Don't save / Cancel", as VS Code does. After the last tab closes, the main area shows its empty state.
- **Actual:** no question. The tab disappears and the text is not written (`DISK has note: false`). The main area keeps a bar with no file name, a grey ●, "Plain text" and an _enabled_ Save button (`03-after-close.png`). Clicking Save or pressing Ctrl+S there does nothing. At step 5 the viewer shows the disk version without her line. Only after Ctrl+E does her text come back, with ●, because it was held in memory (`03-reopen-edit.png`). That memory is lost on reload (NEW-11), so a closed dirty tab and then F5 is permanent loss with no warning at either step. Ctrl+W behaves the same (`03-after-ctrlw.png`). In a real browser (headless mode), Chrome reserves Ctrl+W for closing the browser tab, and the palette shows Ctrl+W as "Close tab". So the keyboard route can close all of Ovra, and with no `beforeunload` that loses everything. The harness can't reproduce this, because Playwright sends keys to the page; this point comes from reading the binding, not from a run.
- **Why it matters for Nadia:** she closes tabs to tidy up, as in VS Code, where a dirty tab always asks. The dead Save button then tells her the editor is broken.
- **Suggested fix:** confirm before closing a dirty tab (Save / Don't save / Cancel) in both `Tabs.close` and `editor.close_tab`. "Don't save" should drop the stash, so a later reopen doesn't bring back text she discarded. In Editor.svelte, reset `session` to null when `openPath` becomes null, so the empty state shows. In headless mode, don't advertise Ctrl+W, or bind an alternative.

### NEW-13 blocker: Unsaved text in a background tab is discarded without a word when the file changes on disk

- **Area:** ui/src/editor (session.svelte.ts `load`: "Unsaved text survives switching files, unless the file changed meanwhile")
- **Steps:** 1. Open `README.md` and `Cargo.toml`. 2. On the `README.md` tab press Ctrl+E and type "Paragraph Nadia wrote.". 3. Click the `Cargo.toml` tab. `README.md` still shows ● ("unsaved changes"). 4. Change `README.md` on disk, as an agent or a `git pull` would (the script appended a line). 5. Click the `README.md` tab.
- **Expected:** her paragraph is still there, together with the conflict notice ("README.md changed on disk while you had unsaved changes: Compare / Keep mine / Take theirs").
- **Actual:** the ● stays on the background tab after the change (`08-background-changed.png`). When she returns, the editor shows the disk version with the agent's line, her paragraph is gone, there is no ●, and there is no alert (`08-back-on-readme.png`: `has Nadia paragraph=false dirty=0 alerts=[]`).
- **Why it matters for Nadia:** Ovra's promise is "agents do the work". Agents write files while she edits, so this path is normal use, not an edge case. She is told nothing, and she can't tell that she lost anything.
- **Suggested fix:** when the stash's mtime differs, load the stash anyway and set `conflict = true`, so she sees the existing "changed on disk" notice. Never drop a stash without a user decision.

### NEW-14 major: Opening a 21st file silently closes the oldest tab, even one with unsaved changes

- **Area:** ui/src/editor (open-files.svelte.ts `MAX_OPEN_FILES`)
- **Steps:** 1. Edit `README.md` (●). 2. Click 21 other files in the tree, one after another (`notes/n01.txt` ... `n21.txt`).
- **Expected:** a dirty tab is never closed automatically. The limit skips dirty tabs, or asks first.
- **Actual:** the tab strip holds 20 tabs, `n02.txt` ... `n21.txt`. `README.md` was dropped without notice (`07-many-tabs.png`), and its text survives only in memory until the next reload (NEW-11). The strip is 1681 px wide in a 1020 px area and scrolls sideways with no hint. Every click from the tree opens a permanent tab (there is no preview tab), so reaching 20 tabs takes one browsing session.
- **Why it matters for Nadia:** clicking around to "find a file and read it" is exactly her session, and it quietly costs her an edit.
- **Suggested fix:** exclude dirty tabs from the limit. Consider a VS Code-style preview tab for single clicks, so browsing doesn't pile up tabs.

### NEW-15 major: There is no visible way to start editing; Ctrl+E is the only route, and a tree click silently switches back to view mode

- **Area:** ui/src/viewers (Viewer.svelte), ui/src/editor, ui/src/tree (Tree.svelte calls `openFile(path)` with the default mode "view")
- **Steps:** 1. Open `src/main.rs` from the tree. 2. Look for an Edit button, double-click the file, double-click the text, right-click the tree row. 3. Open the palette and type "edit". 4. Press Ctrl+E, then click `Cargo.toml` in the tree. 5. Instead, click a tab.
- **Expected:** a visible "Edit" button or a pencil in the view header. Double-click or typing into the text starts editing. The mode stays the same whichever way she switches files.
- **Actual:** the main area of a viewer contains only the tab buttons (`BUTTONS in main` lists only tabs and close buttons). Double-click shows the same view (`02-dblclick.png`), and right-click does nothing (`02-rightclick.png`). The only way is the palette entry "Toggle editing the open file Ctrl+E". After Ctrl+E, a click in the tree goes back to view mode (`MODE after tree click: view`), but a click on a tab keeps edit mode (`TAB click keeps mode: edit`). "Save file" appears in the palette only while the editor is mounted, so in view mode searching for "save" gives "No matching commands".
- **Why it matters for Nadia:** she is mouse first and doesn't know Ctrl+E (in VS Code it opens quick open). She will think Ovra is read-only. The two different mode rules also make her own clicks unpredictable.
- **Suggested fix:** add an "Edit" button to the view header (and "Preview" in the editor bar) bound to `editor.toggle`. Keep the current mode for tree clicks too, or always open code files in the editor. Register "Save file" globally, disabled when there is nothing to save.

### NEW-16 major: Conflicts surface only on Save, and both choices throw work away without saying which

- **Area:** ui/src/editor (Editor.svelte notice, session.svelte.ts)
- **Steps:** 1. Edit `src/lib.rs` and save with Ctrl+S (worked in 73 ms, ● cleared). 2. Type another line. 3. Change the file on disk. 4. Wait: nothing appears. 5. Click Save. 6. Click "Reload from disk". 7. Repeat steps 2-5 and click "Overwrite". 8. Type, delete the file on disk, press Ctrl+S.
- **Expected:** a notice appears as soon as the file changes under her unsaved edit. The two buttons say what they discard ("Discard my changes", "Replace their version"), with a way to compare first. A file deleted on disk says so.
- **Actual:** nothing changes until she saves (`05-dirty-external-change.png`). Then the notice reads "The file changed on disk. Overwrite / Reload from disk" (`05-conflict.png`). "Reload from disk" removes her line immediately, with no confirmation and no undo hint (`has nadia second edit=false`). "Overwrite" replaces the other change without showing it. After the file is deleted on disk, the editor shows no notice, and Ctrl+S quietly creates the file again (`05-deleted-while-dirty.png`, `exists=true`).
- **Why it matters for Nadia:** "Reload from disk" sounds like a refresh, not "delete what you typed". She reads buttons literally.
- **Suggested fix:** set `conflict` from the `fs.changed` handler when the session is dirty, not only on save. Rename the buttons to "Keep my version (overwrite)" and "Discard my changes". Add "Compare" when a diff view exists. Show "This file was deleted on disk" with "Save again" / "Close".

### NEW-17 major (round 1 NEW-3, still open): No way back to the project list, no switching or removing, and a reload forgets the open project

- **Area:** ui/src/shell (header), ui/src/tree (ProjectList), ui/src/palette
- **Steps:** 1. Add `empty`, reload, add `small`, expand `src` and `docs`, open two files. 2. Look for a way back: click the project name in the header, press Alt+Left, open the palette and type "project", "switch", "close". 3. Look for a way to remove the wrongly added `empty`. 4. Reload.
- **Expected:** a clickable project name or a "Switch project" / "Close project" command, a remove action on the start screen, and the last project reopened on reload.
- **Actual:** the header has 0 buttons, and Alt+Left does nothing. The palette has 5 commands (Close tab, Next tab, Previous tab, Show command palette, Toggle editing), none of them about projects (`01-palette.png`). The start screen has no remove control (0 matches), although the core offers `projects.remove`. A reload always lands on the start screen (`01-after-reload.png`). What does work: clicking `demo-app` again restores the expanded folders (`docs`, `src`), the tabs and the active file (`06-restored.png`). Edit mode is not restored, which is fine.
- **Why it matters for Nadia:** a wrong folder is now permanent in her list, and the only "way back" is F5, which costs her unsaved text (NEW-11).
- **Suggested fix:** as in round 1: make the header project name a button that opens the project list, add "Switch project…" / "Close project" commands, add a remove button (×) on each start-screen entry, and reopen the last project on load. That last part needs the workspace-state task.

### NEW-18 major (round 1 NEW-2, still open): Start screen errors still show raw codes and OS text

- **Area:** ui/src/tree (ProjectList), crates/store / crates/core path messages
- **Steps:** on the start screen enter `~/demo-app`, `demo-app`, the path of `README.md`, `/no/such/folder`.
- **Expected:** "Use the full path to a folder, e.g. /home/you/demo-app" (or `~` expanded), "That's a file. Choose the folder that contains it.", "Folder not found: /no/such/folder".
- **Actual:** `bad_request: project root must be an absolute path` (twice), `bad_request: …/README.md is not a folder`, and `not_found: /no/such/folder: No such file or directory (os error 2)` (`01-try-_no_such_folder.png`). The wording after the code is better than in round 1, but the `code:` prefix and "(os error 2)" remain. The old error stays while she types. Nothing has focus on the first screen (`FOCUS: <body>`). A trailing slash works.
- **Why it matters for Nadia:** "bad_request" and "os error 2" read as "you broke it".
- **Suggested fix:** show `error.message` without the code, strip OS suffixes in the core, add a hint line under the field, clear the error on input, and autofocus the field.

### NEW-19 minor: "Open in external editor" has no entry point, and web links give no feedback

- **Area:** ui/src/viewers, ui/src/editor (no caller of `open.external`), ui/src/viewers/MarkdownView.svelte (`open.url` errors swallowed)
- **Steps:** 1. Look for "Open in…" on a file, tab, viewer or palette. 2. In `docs/links.md` click "Rust site" (https) and "mail" (`mailto:`).
- **Expected:** an "Open in default app" action, which matters most where Ovra can't show the file: binary, too large. A link click shows "Opening in your browser…", or an error if that fails.
- **Actual:** no UI calls `open.external`. The https link sends `open.url`. The core answers `unavailable: could not start a program to open it (xdg-open: …); set $VISUAL to a graphical editor` (frame id 34), and the UI drops it with `.catch(() => {})`: nothing happens on screen (`02-link-Rust_site.png`). In the container no opener exists, so the failure is expected; the silence is the finding. `mailto:` does nothing at all. The "Binary file (application/octet-stream) cannot be shown." and "This file is too large to show." messages offer no next step and no size.
- **Suggested fix:** an "Open in default app" button on binary / too-large views and in the tab context. A short toast for `open.url` failures, with the message translated, not the `$VISUAL` hint, which is meant for editors.

### NEW-20 minor: Preview of a file with unsaved changes shows the disk version without saying so

- **Area:** ui/src/viewers (Viewer.svelte)
- **Steps:** edit `README.md` (add a line), press Ctrl+E to switch to view.
- **Expected:** the preview shows her text, or a banner "Showing the saved version; you have unsaved changes".
- **Actual:** the rendered markdown lacks her line. The only hint is the ● on the tab (`03-view-while-dirty.png`).
- **Suggested fix:** render the editor buffer in view mode when the file is dirty, or show the banner.

### NEW-21 minor: A broken link opens a tab that says the file "no longer exists"

- **Area:** ui/src/viewers (load.ts message, MarkdownView link handling)
- **Steps:** in `docs/links.md` click `[missing file](./nope.md)`.
- **Expected:** "docs/nope.md doesn't exist", without opening a tab, or a tab with a "Create file" option.
- **Actual:** a permanent tab `nope.md` opens with red "docs/nope.md no longer exists." (`02-link-missing_file.png`). It never existed, and the tab stays and counts towards the 20-tab limit. Links to other project files (`../README.md`, `../src/main.rs`) work well.
- **Suggested fix:** check existence before opening a tab from a link. Use "not found" wording unless the file was open before.

### NEW-22 polish: Small wording and visual items

- **Area:** ui/src/editor, ui/src/viewers, ui/src/tree
- Markdown files are labelled "Plain text" in the editor bar (`03-typed.png`), and the path appears twice (tab and bar).
- Main-area hints ("Viewer: open a file from the tree.", "This file is too large…") still use `#6f6f7a` on `#16161a` (3.64:1). "Viewer:" is an internal word; "Open a file from the tree on the left" is enough.
- The `empty` project still shows a lone dimmed `.git` row and never "Empty project" (`06-empty-project.png`, round 1 NEW-6).
- The 1×1 `logo.png` is shown at 1 px with a "1 × 1" caption; small images could be upscaled with pixelated rendering.

## Round 1 re-check

| Round 1 finding                                                  | Status         | Notes                                                                                                                                                                                      |
| ---------------------------------------------------------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| NEW-1 blocker: adding a project shows `unavailable … (task 1.B)` | **fixed**      | The real store works: add opens the project and the list persists. A trailing slash is accepted.                                                                                           |
| NEW-2 major: raw error codes, no hint                            | **still open** | The message text is clearer, but the `bad_request:` / `not_found:` prefix, "(os error 2)" and the missing hint remain; `~` is not expanded (NEW-18).                                       |
| NEW-3 major: no way back to the project list or to switch        | **still open** | Partly better: reopening a project restores the expanded folders, tabs and active file. There is still no switch, close or remove, and a reload drops the open project (NEW-17).           |
| NEW-4 major: clicking a file shows only "Viewer"                 | **fixed**      | Real viewers for markdown, diff, image, text, binary and too large. The empty state says "Viewer: open a file from the tree." A raw `<script>` in `guide.md` is shown as text and not run. |
| Minor NEW-5 (palette)                                            | changed        | 5 commands now, but none for projects or saving in view mode.                                                                                                                              |
| Minor NEW-6, NEW-8, NEW-9                                        | still open     | Lone `.git` row, no focus on the first screen, low-contrast hints.                                                                                                                         |

## What worked well

- Reading is good: README renders with headings, task-list checkboxes and code blocks. The diff view has old/new line numbers and red/green rows (`02-patch.png`). The binary and too-large files get one plain sentence instead of garbage or a freeze.
- Relative links between markdown files and into source files open the right file. Anchor links scroll, and a link never navigates the IDE away.
- Saving is quick (Ctrl+S, 73 ms) and the ● clears. When the file is clean, an outside change shows up in the editor by itself.
- Per-project state comes back when the project is reopened: expanded folders, the tabs in order, and the active tab.
- The tab ● and its accessible label "Close README.md (unsaved changes)" are clear. The fixes above only need to make them true.
