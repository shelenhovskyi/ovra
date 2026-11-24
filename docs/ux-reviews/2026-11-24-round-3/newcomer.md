# Nadia, the newcomer: Round 3 (2026-11-24), regression

Build: 9512ea6 (main), headless, real core, Playwright's bundled Chromium (Linux), 1280x800.
Scope: re-check of every newcomer finding from rounds 1 and 2 (blocker, major, minor, and the polish items), plus the features new since 485e5f2: drafts for unsaved text, the close-tab question, conflict and deleted notices, project switcher and Close project, connection status and reconnect, browser-safe shortcuts (Alt+W, Alt+], Alt+[, Alt+P), Go to file over `fs.list_files`, virtualized diffs, tree refresh. Fixtures: `small` (scripts add `notes/n01..n21.txt` and `docs/links.md`), `empty`; one run opens `/root` (2.4 GB) as a large real folder.

Screenshots and logs are in the reviewer's scratch folder (`/tmp/claude-0/ux3-newcomer/<run>/`), named here as `<run>/<file>`. New finding IDs start at NEW-23, because NEW-19 to NEW-22 already exist in round 2.

## Summary

Nadia can now do her whole session without help. She opens a folder with plain error messages, finds a file with Go to file, reads it, edits it, and no longer loses text. All three round-2 blockers are fixed: drafts survive a reload, a dirty tab asks Save / Don't save / Cancel, and a background change gives a conflict notice. The remaining problems are about finding things and recovering. Editing still has no visible button. Ctrl+P, the one shortcut she knows from VS Code, does nothing in the browser. A reload still drops her on the start screen. When the core restarts, the open page keeps saying "reconnecting…" forever and never tells her to use the new address. Overall the app now feels safe, and it is calm whenever the core is running.

Counts over 22 earlier findings: 15 fixed, 6 partly fixed, 1 still open, 0 regressed. New: 0 blocker, 1 major, 5 minor, 1 polish.

## Regression table

| ID     | Round | Old severity | Status         | Evidence                                                                                                                                                                                                                                                                                                                                                        |
| ------ | ----- | ------------ | -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NEW-1  | 1     | blocker      | fixed          | Adding `/root` and both fixtures works on the real core, and no "task 1.B" text appears anywhere (s1, s2).                                                                                                                                                                                                                                                      |
| NEW-2  | 1     | major        | fixed          | `/no/such/folder` gives "Folder not found: /no/such/folder", a file gives "That is not a folder. Choose the folder itself, not a file in it.", and `~/demo-app` gives "Use the full path to a folder, for example /home/you/my-app." No code prefix and no "(os error 2)" (`s1/02-try-_no_such_folder.png`). `~` is still not expanded, but the hint covers it. |
| NEW-3  | 1     | major        | partly fixed   | See NEW-17.                                                                                                                                                                                                                                                                                                                                                     |
| NEW-4  | 1     | major        | fixed (stays)  | Real viewers. The empty main area says "Open a file from the tree on the left."                                                                                                                                                                                                                                                                                 |
| NEW-5  | 1     | minor        | partly fixed   | The palette has 11 commands, including Go to file, Switch project, Close project, Collapse all and Reveal. Still nothing happens on Ctrl+P or F1, and "save" and "help" give "No matching commands" in view mode (s3). See NEW-25.                                                                                                                              |
| NEW-6  | 1     | minor        | fixed          | The `empty` project shows "Empty project" and no `.git` row (`s3/02-empty-project.png`).                                                                                                                                                                                                                                                                        |
| NEW-7  | 1     | minor        | fixed          | Start screen entries show name and path on two lines under "Recent". The field label is "Open a folder", and the button is "Open" (`s3/01-start-two.png`).                                                                                                                                                                                                      |
| NEW-8  | 1     | minor        | fixed          | The field has focus on the first screen (`FOCUS: <input id="project-root">`), the error clears on the first keystroke, and the button shows "Opening…" and is disabled while busy.                                                                                                                                                                              |
| NEW-9  | 1     | polish       | partly fixed   | The start hint is `#9a9aa6` (6.48:1) and the viewer hint `#8a8a96` (5.29:1). The palette's "No matching files/commands" and the editor `.message` still use `#6f6f7a` (3.64:1).                                                                                                                                                                                 |
| NEW-10 | 1     | polish       | fixed          | Tree rows carry `title` with the full name (`very-long-directory-name-that-will-not-fit-in-the-sidebar`). Folder dots read "contains modified files".                                                                                                                                                                                                           |
| NEW-11 | 2     | blocker      | fixed          | Edit README.md and Cargo.toml, then reload and reopen: both tabs come back with ●, and README's preview shows "TODO from Nadia…" under "Showing unsaved changes from the editor." (`s4/04-readme-view-after-reload.png`).                                                                                                                                       |
| NEW-12 | 2     | blocker      | fixed          | × on a dirty tab shows "Save changes to README.md before closing? Save / Don't save / Cancel" with focus on Save, and Esc cancels. "Don't save" really drops the text (reopen shows no note). Alt+W asks the same question (`s4/05-close-dialog.png`, `s4/06-altw.png`).                                                                                        |
| NEW-13 | 2     | blocker      | fixed          | Her paragraph survives an outside change to a background tab. On return: "README.md changed on disk while you have unsaved changes. Overwrite with my version / Discard my changes and reload" (`s5/02-back-readme.png`).                                                                                                                                       |
| NEW-14 | 2     | major        | fixed          | After 21 files, the tab strip holds 20 tabs and the dirty `README.md` is kept as the first one (`hasReadme=true`). The leftover overflow problem is NEW-28.                                                                                                                                                                                                     |
| NEW-15 | 2     | major        | **still open** | The only button in a viewer is "Open in default app". Double-click stays in view mode, and a tree click after Ctrl+E drops back to view (`mode after tree click: view`), while tab clicks keep the mode.                                                                                                                                                        |
| NEW-16 | 2     | major        | partly fixed   | A notice appears within 105 ms of an outside change while the file is open. The buttons say what they discard. A deletion shows "src/main.rs was deleted on disk. Save as a new file / Close without saving", and Ctrl+S no longer recreates the file (`exists=false`). There is still no Compare, although a diff view exists now.                             |
| NEW-17 | 2     | major        | partly fixed   | "Switch project" and "Close project" work from the palette and restore tabs and drafts (s3, s9). Still: the header name is plain text (0 buttons), the start screen has no remove control (0), and a reload inside a project lands on "No project" (s9).                                                                                                        |
| NEW-18 | 2     | major        | fixed          | Same evidence as NEW-2.                                                                                                                                                                                                                                                                                                                                         |
| NEW-19 | 2     | minor        | fixed          | "Open in default app" sits on every view, including binary and too large. A failed https link says "Could not open https://www.rust-lang.org in a browser.", and `mailto:` says "This link cannot be opened from Ovra." (s6).                                                                                                                                   |
| NEW-20 | 2     | minor        | fixed          | Preview of a dirty file shows the editor text with the line "Showing unsaved changes from the editor."                                                                                                                                                                                                                                                          |
| NEW-21 | 2     | minor        | fixed          | A broken link stays on `links.md` with the status "The link points to docs/nope.md, which was not found." and opens no tab.                                                                                                                                                                                                                                     |
| NEW-22 | 2     | polish       | partly fixed   | "Viewer:" is gone and "Empty project" shows. Markdown is still labelled "Plain text" in the editor bar, and `logo.png` is still drawn 1×1 px with `image-rendering: auto` (s11).                                                                                                                                                                                |

## New findings

### NEW-23 major: After the core restarts, the open page says "reconnecting…" forever, and a reload says "could not connect"

- **Area:** ui/src/lib/transport.ts and ui/src/shell (status text); crates/daemon (token per run)
- **Steps:** 1. Open `demo-app`, edit README.md (●). 2. Stop the core (killed in the script; for her this is closing the terminal or a crash). 3. Start `ovra-daemon` again with the same port and data folder. 4. Wait. 5. Reload the page.
- **Expected:** the page notices that the core is back but will not accept this page. It says so in plain words: "Ovra was restarted. Open the new address printed by ovra-daemon; your unsaved text is kept." Or the restarted core accepts the page again.
- **Actual:** the status bar stays on red "Connection to core lost, reconnecting…" (`s8/04-after-restart.png`). The console shows a retry about every second, each answered with `HTTP Authentication failed; no valid credentials available`, because the old token is in the URL. After a reload, the start screen shows "connection to core closed" in red and "Core unavailable: could not connect, retrying…". The "Open a folder" field stays enabled (`s10/01-old-url.png`). Only opening the new URL helped, and then her draft was still there (`text kept=true`, `s8/05-new-url.png`). With the default `--port 0`, a restart even changes the port, so the old tab can never recover.
- **Why it matters for Nadia:** "retrying…" tells her to wait, so she waits forever. Nothing on screen says the fix is a new address, and she never read the docs where that might be explained.
- **Suggested fix:** treat a rejected handshake (auth failure) as a final state, not a retry. Show a full-width message that tells her to open the address ovra-daemon printed, and disable the start-screen form while there is no core. Keeping the token stable across restarts (in the data folder) would remove the problem, but it touches headless-mode access checks and needs the owner and a security review.

### NEW-24 minor: While the core is down, every panel shows the raw text "connection to core closed", and Save says nothing about her text

- **Area:** ui/src/editor (Editor.svelte notice), ui/src/viewers, ui/src/palette (error text from `CoreError`/transport)
- **Steps:** 1. Edit README.md. 2. Stop the core. 3. Press Ctrl+S. 4. Click `Cargo.toml` in the tree. 5. Press Alt+P.
- **Expected:** one friendly message, for example "Not connected to Ovra's core. Your unsaved text is kept in this browser; save again when it reconnects." Save is disabled while the core is gone, and a tree click does not open a tab.
- **Actual:** the editor shows a brown bar with lowercase "connection to core closed", and Save stays enabled (`s8/02-save-while-down.png`). The tree click opens a new `Cargo.toml` tab whose body says "connection to core closed". The palette shows the same sentence twice. The status bar says "reconnecting", while the panels say "closed".
- **Why it matters for Nadia:** she reads "closed" literally and thinks her edit is gone. In fact it is safe in drafts, but nothing tells her that.
- **Suggested fix:** map the transport's closed error to one human sentence in a shared place and mention that drafts are kept. Disable Save and file loads while the status is not `open`.

### NEW-25 minor: Ctrl+P, the shortcut she knows, does nothing in the browser; the palette advertises Alt+P instead

- **Area:** ui/src/keys (shortcut.ts `BROWSER_SAFE`)
- **Steps:** 1. Open `demo-app`. 2. Press Ctrl+P, then F1. 3. Open the palette with Ctrl+Shift+P and type "file".
- **Expected:** Ctrl+P opens Go to file, as in VS Code. F1 opens the palette.
- **Actual:** nothing opens on Ctrl+P or F1 (`after Ctrl+P dialogs=0`, `after F1 dialogs=0`; `s3/03-ctrl-p.png`). In a real Chrome tab, Ctrl+P then opens the print dialog. The palette lists "Go to file Alt+P". Unlike Ctrl+W and Ctrl+Tab, Ctrl+P is one of the keys a page _can_ take over with `preventDefault()` (web editors do this), so the stand-in costs her the familiar key for no gain.
- **Why it matters for Nadia:** Ctrl+P is the one VS Code shortcut she half-remembers. A print dialog teaches her that Ovra has no file search, although Go to file works very well (435 ms from key to open file).
- **Suggested fix:** drop `Mod+P` from `BROWSER_SAFE` and keep Alt+P as a second binding if wanted. Bind F1 to "Show all commands".

### NEW-26 minor: Go to file ranks scattered matches too high, finds nothing on a small typo, and silently skips ignored files that the tree shows

- **Area:** ui/src/palette (fuzzy.ts, sources.ts)
- **Steps:** in Go to file type `main`, `readme`, `mian`, `build.log`.
- **Expected:** `main` and `readme` give one sensible hit. `mian` still finds `src/main.rs`. `build.log`, which she sees dimmed in the tree, is found, or the empty state says why not.
- **Actual:** `main` gives `src/main.rs` and also `very-long-directory-name-…/and-a-long-file-name-too.config.json`, and `readme` gives README.md plus the same long path. `mian` gives _only_ the long path. `build.log` gives "No matching files". The count reads "1 results".
- **Why it matters for Nadia:** a wrong top hit after a typo makes her think the file doesn't exist, and a file she can see in the tree but cannot find makes her distrust the search.
- **Suggested fix:** penalise matches spread across many path segments, or require a match in the file name for short queries. Allow one transposition. For no results, say "Ignored files (for example build.log) are not listed; open them from the tree." Fix the plural.

### NEW-27 minor: Opening a large folder shows only "Opening…" for about 6 seconds, with "No projects yet." above it

- **Area:** ui/src/tree (ProjectList), crates/fs-index (`projects.open` waits for the file list)
- **Steps:** on the start screen type `/root` (2.4 GB with caches and `.cargo`) and press Enter. This is a realistic mistake: her home folder instead of the project.
- **Expected:** the project appears in the list at once, and the tree shows within a second, filling in as folders load. If something has to wait, the screen says what ("Reading files in /root…") and offers Cancel.
- **Actual:** `projects.add` returned in 2 ms, but `projects.open` took 5.4 s, and the whole open took 5842 ms. All that time the sidebar reads "No projects yet." over a disabled "Opening…" button (`s1/03-after-root.png`). Nothing says it is working on a big folder, and she cannot cancel.
- **Why it matters for Nadia:** six silent seconds after her first click reads as a hang. "No projects yet." contradicts what she just did.
- **Suggested fix:** don't block `projects.open` on the full file list (build it in the background; Go to file can show "Indexing…"). Update the Recent list as soon as `projects.add` succeeds. Show the folder being opened in the busy text.

### NEW-28 minor: Unsaved work can be out of sight: a dirty tab scrolls off the strip, and switching or closing a project with unsaved text says nothing

- **Area:** ui/src/editor (Tabs.svelte), ui/src/palette (switch / close project), ui/src/tree (ProjectList)
- **Steps:** 1. Edit README.md, then open 21 more files. 2. Look at the tab strip. 3. Use "Switch project" or "Close project" from the palette. 4. Look at the start screen.
- **Expected:** an overflow hint on the strip ("‹ 1 more, 1 unsaved"). Switching or closing a project with unsaved text says "1 file with unsaved changes is kept for demo-app". The start screen marks that project with ●.
- **Actual:** the strip is 1715 px wide in 1020 px, scrolled to the end, so the only dirty tab is hidden on the left with no hint (`s6/01-many-tabs.png`). Switch and Close project happen without a word (alerts `[]`), and the start screen shows no mark. The text is kept (drafts restore it on return), so nothing is lost, but she can't see that.
- **Why it matters for Nadia:** after round 2 she has learned to distrust editing. Unsaved work that is invisible makes her wonder whether it still exists.
- **Suggested fix:** add a dirty count or overflow arrows to the tab strip, a one-line toast on switch or close when drafts exist, and a ● on start-screen entries with drafts.

### NEW-29 polish: Small items

- **Area:** ui/src/shell, ui/src/viewers, ui/src/editor
- The header project name is still plain text. Making it the Switch project button is the most obvious "way back" for a mouse user.
- With no project open, the main area says "Open a file from the tree on the left.", but there is no tree, only the project list. In `empty` it says the same next to "Empty project".
- The white "Open in default app" button is the most prominent control in every viewer. Without an Edit button (NEW-15), it looks like the way to edit.
- The empty palette text and editor messages are still `#6f6f7a` (3.64:1). In the editor bar, markdown is still "Plain text". The 1×1 logo is still drawn at 1 px.

## What worked well

- Unsaved text is safe in every path tested: reload, closed dirty tab (now asks), 21st file, background change, outside change while open, deletion, project switch, and core loss. Each time the text came back or a clear notice appeared.
- The close question is well made: three plain buttons, focus on Save, Esc cancels, and "Don't save" really discards.
- The start screen is now self-explanatory: field focused, hint under it, human error messages that clear while she types, path under each recent project, "Empty project" for an empty repo.
- Go to file is fast and pleasant: 15 files listed at once, matches highlighted, a new file shows up within a second, and the tree reveals the opened file.
- Link and opener feedback replaces silence: missing files, failed browser opens and `mailto:` each get one sentence, and binary and too-large views state size and type and offer "Open in default app".
- The tree updates live: a new file appears with U, a deleted one disappears, and folder dots explain themselves.
