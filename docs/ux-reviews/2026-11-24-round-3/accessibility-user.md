# Iryna (accessibility user): Round 3 (2026-11-24), regression

Build: 9512ea6 on main, headless, real core (debug build), Playwright
Chromium. Viewports: 1280x800, and 512x384 CSS px (200% zoom of a 1024 px
window). Forced colours on and off, 26 px root text.
Scope: re-check of every A11Y finding from rounds 1 and 2, plus what landed
since 485e5f2: the tree's screen-reader model, the connection status, unsaved-change
prompts and drafts, the project switcher, the file finder, virtualized diffs
and text, browser-safe shortcuts. Fixtures: `small` (seeded), `empty` for
switching, and a 3-file, 3,000-line `big.patch` that the script writes.
Scripts s1-s10 live in a scratch folder.

## Summary

This round is a big step forward for Iryna. The file tree now reads every row
with its level, position among siblings, expanded state and git status. Closing a
dirty tab asks first in a named alertdialog. Unsaved text survives reloads and
project switches. The status bar announces "Connection to core lost,
reconnecting…" and then "Connected". The patch view speaks "removed line 2:"
and "added line 2:". One new blocker came with these changes: expanding a
folder while the core is disconnected freezes the whole page. The tab stops
responding to keys and to the screen reader, and only a reload gets it back.
The rest is smaller. The close prompt does not trap focus. A save is still
silent. Focus is still lost in a few places (Escape then Tab out of the
editor, Close project). The editor textbox has no name. Virtualized diffs show
only about 50 rows to browse mode, with no hint that more exist. Score: 8
fixed, 9 partly fixed, 3 still open, 0 regressed.

## Regression table

| ID      | Round | Old severity | Status     | Evidence (round 3)                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------- | ----- | ------------ | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A11Y-1  | 1     | blocker      | fixed      | The tree keeps focus and sets `aria-activedescendant`, which follows each key: ArrowDown gives `assets lvl=1 exp=false pos=1/14`, then `src … pos=5/14 desc=contains modified files`, End gives `README.md … 14/14`. The active row is always rendered.                                                                                                                                                                                                   |
| A11Y-2  | 1     | major        | fixed      | Position and set size count siblings: `util 1/4`, `main.rs 3/4`, `debug 1/1`. Twisty, badge and "…" are `aria-hidden`, so names are clean (`treeitem "src"`). Status comes as a description: "modified", "untracked", "ignored", "loading".                                                                                                                                                                                                               |
| A11Y-3  | 1     | major        | partly     | Ignored entries are now italic, #9a9aa6 (6.48:1), and described as "ignored". Files keep the M/U letters with a spoken description. Folders still show a "•" whose colour alone gives the kind of change. In forced colours every dot looks the same (`fc-tree.png`). The description names one kind only: `src` says "contains modified files" although it also holds an untracked file.                                                                 |
| A11Y-4  | 1     | major        | fixed      | Footer is `role="status"`. Status log: `Connected…` → `Connection to core lost, reconnecting…` (red, #ff6b6b) → `Connecting…` → `Connected…`. Ctrl+S while offline shows `role="alert"` "connection to core closed" at once. That alert stays after the reconnect, until the next save (polish). See the new blocker A11Y-21 for the tree.                                                                                                                |
| A11Y-5  | 1     | major        | partly     | After Enter on a project, focus is on `DIV role=tree name=Files`. But `aria-activedescendant` is null until the first arrow key, so the reader says only "Files, tree". Nothing says "demo-app opened". The header's project name is not live.                                                                                                                                                                                                            |
| A11Y-6  | 1     | major        | fixed      | Forced colours: the selected option gets a Highlight fill and outline, and matched letters are underlined (`fc-palette.png`).                                                                                                                                                                                                                                                                                                                             |
| A11Y-7  | 1     | minor        | partly     | Shared greys are now 5.3-6.5:1: footer, hint and project path #9a9aa6 6.48:1, viewer notes #8a8a96 5.29:1, ignored rows 6.48:1. Still low: palette "No matching commands" #6f6f7a 3.34:1, the editor's "No file open" message #6f6f7a. The selected but unfocused tree row still differs from the background only by its fill (#2a2d3a on #16161a), and in forced colours it does not show at all (`fc-tree-unfocused.png`).                              |
| A11Y-8  | 1     | minor        | partly     | The tree now has an inset 1 px #4c6ef5 ring when focused, and the focused row a 1 px outline that forced colours keep (`fc-tree-kbd.png`). There is still no current row when the tree gets focus right after a project opens (see A11Y-5).                                                                                                                                                                                                               |
| A11Y-9  | 1     | minor        | fixed      | Visible label "Open a folder" and a hint. `aria-invalid=true` and `aria-describedby="project-root-error project-root-hint"` are set. Plain error "Folder not found: /does/not/exist". The project path shows as text under the name (6.48:1).                                                                                                                                                                                                             |
| A11Y-10 | 1     | minor        | still open | `html, body { font-size: 13px }` still overrides the user's font size. With 26 px text, tree rows stay 22 px high (`text-200.png`). The sidebar is still a fixed 260 px, so at 512 px it takes half the width. Zoom itself holds: `scrollW=512 clientW=512`.                                                                                                                                                                                              |
| A11Y-11 | 1     | minor        | fixed      | The palette has a hidden `role="status"`: "2 results", then "No matching commands" for `zzz`. `aria-autocomplete="list"` is still missing on the combobox (polish).                                                                                                                                                                                                                                                                                       |
| A11Y-12 | 1     | polish       | still open | The start screen still shows heading "File tree" directly before "Projects". `aside` has no name, `main` has no heading, and there is no key to move between regions (F6).                                                                                                                                                                                                                                                                                |
| A11Y-13 | 2     | blocker      | fixed      | Alt+W or Delete on a dirty tab opens `alertdialog "Save changes to lib.rs before closing?"` with Save / Don't save / Cancel. Focus starts on Save, and Escape returns focus to `tab "lib.rs, unsaved changes"`. A reload keeps the text as a draft: on reopen, the tab reads "lib.rs, unsaved changes" and the view shows `// a comment RELOAD-DRAFT`. A project switch keeps it too. See A11Y-22 for gaps in the dialog.                                 |
| A11Y-14 | 2     | major        | partly     | Fixed: after Ctrl+S and after the Save button, focus stays in the editor textbox. Ctrl+E focuses the editor. Arrow keys and Escape in the dialog keep focus on tabs. Alt+] moves focus to the new content. Still lost: Escape then Tab from the editor lands on `<body>`. "Close project" lands on `<body>`. Opening a file from the file finder (Alt+P, Enter) sends focus back to the tree, not to the file.                                            |
| A11Y-15 | 2     | major        | partly     | Fixed: the tab name is "lib.rs, unsaved changes", and the editor bar's ● carries hidden text "Unsaved changes". Still open: a successful save announces nothing (live regions before and after Ctrl+S: `polite:"" , status:"Connected…"`). "Loading…", the binary and too-large messages, and "Showing unsaved changes from the editor." are plain `<p>`, not live.                                                                                       |
| A11Y-16 | 2     | major        | partly     | Fixed: roving `tabindex`; ArrowLeft, Home and Delete work; `aria-controls="ovra-tabpanel"` points at a `tabpanel` named after the file; the active tab scrolls into view at 512 px (`tab 260..305 list 260..512`); one Tab stop for the strip. Still open: close buttons are children of the `tablist`, and they are 20x20 px. In forced colours the active tab's Highlight underline is hard to tell from the strip's black border (`fc-tabs-crop.png`). |
| A11Y-17 | 2     | major        | fixed      | Each row is spoken with its kind: `2 removed line 2: a + b`, `2 added line 2: a.wrapping_add(b)`. The sign is `aria-hidden`. Forced colours add a solid or dashed left border. File headers are `button [expanded]` with "+1 −1". Line numbers are 4.57:1 on the red row. Polish: the 3 px solid and dashed borders are hard to tell apart, and the focused region's 2 px outline covers them (`fc-patch-crop.png`).                                      |
| A11Y-18 | 2     | minor        | partly     | The image figure is now `figure "Image logo.png"` with `img "logo.png"`. Still open: the CodeMirror textbox has no name (`role=textbox aria-multiline=true`, no label). There is no visible View/Edit toggle in the viewer or editor bar. Escape then Tab leaves the editor to `<body>`.                                                                                                                                                                  |
| A11Y-19 | 2     | minor        | partly     | Viewer notes are 5.29:1 and patch numbers 4.57:1 (fixed). Still low: gutter line numbers #5a5a66 2.65:1, comments #6a737d 3.48:1 on the active line. Buttons still use Chromium's default `auto 1px rgb(16,16,16)` focus ring on near-black.                                                                                                                                                                                                              |
| A11Y-20 | 2     | minor        | still open | Task-list checkboxes are still unnamed (`checkbox [disabled]`, then the text "item" as a separate node). The `article` still has no name. Anchor jumps were not re-tested.                                                                                                                                                                                                                                                                                |

Totals over the 20 IDs: fixed 8 (1, 2, 4, 6, 9, 11, 13, 17), partly fixed 9
(3, 5, 7, 8, 14, 15, 16, 18, 19), still open 3 (10, 12, 20), regressed 0.

## New findings

### A11Y-21 blocker: expanding a folder while the core is disconnected freezes the page

- **Area:** ui/src/tree (`model.ts`, `pump()` and `due()`), together with the fail-fast `call()` in ui/src/lib/transport.ts
- **Steps:** 1. Open `demo-app`. 2. Stop the core, or close the WebSocket so reconnects fail (s6b-s6d replace `WebSocket` so new sockets go to a dead port). The status correctly shows "Connection to core lost, reconnecting…". 3. Focus the tree and press ArrowDown to a collapsed folder (`docs` or `node_modules`). 4. Press ArrowRight.
- **Expected:** The folder shows "could not load" (or stays collapsed), and a `role="alert"` says the core is not reachable. Once the core is back, the folder loads.
- **Actual:** The page stops responding. In s6d, three ArrowDown presses took 4-6 ms each. The ArrowRight press never returned ("press stuck"), and `page.evaluate` got no answer within 3 s and then 5 s. Screenshots time out. s6 and s6b hung until they were killed. Only one `fs.list_dir` frame appears in the log, so the loop runs inside the page. `call()` rejects at once while the socket is closed. `fetch()` swallows the error, so the folder stays expanded but not loaded. `due()` then returns it again, and `pump()`'s `for (; batch.length > 0; batch = this.due())` loop spins on microtasks forever. Any listing error other than `not_found` probably loops the same way, but connected it would turn into a request storm.
- **Why it matters for Iryna:** her screen reader goes silent with no message, the same as a crash, and she cannot even Tab out. A daemon restart is exactly when she is exploring the tree to find out what happened. Drafts are saved in the background, but a frozen page cannot save anything written after the freeze.
- **Suggested fix:** in `TreeModel`, remember failed directories, for example in a `failed` set that `due()` skips until the next `invalidate*` or an `info.resync`. Collapse the folder, or keep it expanded with an error row. Rebuild once. Unit test: with a loader that always rejects, `expand("docs")` resolves and calls the loader once. Also re-list on reconnect, which should already happen through `info.resync`.

### A11Y-22 minor: the close prompt does not hold focus, and it lingers when focus leaves

- **Area:** ui/src/editor (Tabs.svelte)
- **Steps:** 1. Make `lib.rs` dirty and press Alt+W. 2. Press Tab four times. 3. Press Escape. 4. Open another file from the tree.
- **Expected:** An `alertdialog` that asks a blocking question keeps focus. Tab cycles Save → Don't save → Cancel. Escape works wherever focus is, or the dialog is dismissed when focus leaves. `aria-modal="true"` is set, and the question is the description as well as the name.
- **Actual:** Tab goes Save → Don't save → Cancel → the editor bar's "Save" → the editor textbox, and focus leaves the dialog. Escape pressed in the editor does nothing (`dialogs=1`). The dialog stays on screen above another file (`openPath tab=main.rs`, `dialog-left-open.png`), and a later "Don't save" closes `lib.rs`, which is no longer in view. The dialog has no `aria-modal` and no `aria-describedby`. In forced colours it has no border or fill, so it blends into the editor (`fc-dialog.png`). Good: the role and name are right, focus starts on Save, Escape inside returns focus to the tab, Cancel returns focus to the tab, and it fits at 512 px.
- **Why it matters for Iryna:** after Tab she is back in the text with a question still pending that she cannot hear. The next Escape does nothing, and she has to find her way back to the prompt.
- **Suggested fix:** trap Tab and Shift+Tab inside the prompt (cycle among its three buttons), or cancel on `focusout` to outside the prompt. Add `aria-modal="true"` and `aria-describedby` pointing at the question. Under `forced-colors`, give `.confirm` a `1px solid CanvasText` border.

### A11Y-23 major: virtualized diffs and text show only the rendered rows to a screen reader

- **Area:** ui/src/viewers (Lines.svelte, PatchView.svelte)
- **Steps:** 1. Open `big.patch` (3 files, 3,000 changed or unchanged lines). 2. Read the tab panel in browse mode (ariaSnapshot), then press PageDown in the focused region.
- **Expected:** The reader knows how long the content is and can reach every line. For example: rows exposed as `role="row"` or `listitem` with `aria-rowindex`/`aria-posinset` and `aria-rowcount`/`aria-setsize`; a region description such as "3,000 lines, showing 94-157"; or a "Read as plain text" fallback.
- **Actual:** The `region "Changes"` holds 64 rendered `.line` divs (391 nodes) as one flat run of text, and the text stops at "48 48 unchanged line 48". Nothing says that 2,950 more lines exist. PageDown scrolls (the first rendered line becomes `94 - removed line 94`), but nothing is announced, and rows have no position information. The plain-text viewer uses the same `Lines` component (`region "File text"`). The semantics of each rendered row did survive: rows still say "removed line N:".
- **Why it matters for Iryna:** reviewing an agent's diff is a core task. Browse mode reaches the bottom of the visible window, looks finished, and she may approve a diff she has not fully heard.
- **Suggested fix:** in `Lines`, expose the total on the region (`aria-description="3,000 lines"`) and give each rendered row `role="listitem"` with `aria-setsize`/`aria-posinset` inside a `role="list"`, or give each row its index. Add hunk and file navigation keys (for example `n` and `p`, or the existing "Go to file" select plus "next change") that move focus to a row and announce it. Keep a polite status such as "Lines 94-157 of 3,000" after a PageDown.

### A11Y-24 minor: project switch and close are silent; Close project drops focus

- **Area:** ui/src/palette (sources.ts `openProject`), ui/src/shell (header)
- **Steps:** 1. Ctrl+Shift+P, "Switch project", choose `empty`. 2. Switch back to `demo-app`. 3. Ctrl+Shift+P, "Close project".
- **Expected:** "Opened empty" (or "Closed demo-app") is announced, and focus lands on the tree's current row or on the project list.
- **Actual:** Switching moves focus to `tree "Files"`, which is good. But for `empty`, the tree contains only `paragraph: Empty project` and no item, and nothing announces the change: the header's project name is not live. "Close project" leaves focus on `<body>`. The palette dialog is still called "Command palette" in its "Switch to project" and "Go to file" modes. Unsaved text is kept across the switch (good).
- **Suggested fix:** announce project changes through the shell's status (for example "Opened demo-app" for a few seconds, or a separate polite region). After "Close project", focus the first project button. Name the palette dialog after its mode ("Go to file", "Switch project"). Show the empty state outside the `tree` role, or as a disabled treeitem.

### A11Y-25 polish: the file finder sends focus back to the tree instead of the opened file

- **Area:** ui/src/palette (Palette.svelte focus return), ui/src/viewers
- **Steps:** 1. With the tree focused, press Alt+P, type `changes.patch`, and press Enter.
- **Expected:** Focus moves to the opened document (the viewer's content or the editor), as with Alt+].
- **Actual:** `focus after open patch: DIV role=tree … ad=…-13`. The tree's active row follows the file, which is helpful, but reading the file takes Tab, Tab, Tab (tabs, "Open in default app", region).
- **Suggested fix:** when a palette entry opens a file, skip restoring focus and focus the content instead.

### Note on browser-safe shortcuts

In headless mode the palette lists the shortcuts that really work, such as
"Close tab Alt+W", "Go to file Alt+P" and "Next tab Alt+]". Ctrl+P no longer
does anything in the app, so the browser's print dialog takes it, as
intended. Alt+letter shortcuts can clash with screen reader and menu keys on
Windows (NVDA uses Insert or CapsLock, so Alt+W is fine in Chrome). This is
not a finding. The palette is the way to discover the shortcuts, and it reads
them correctly.

## What worked well

- The file tree is now a correct WAI-ARIA tree: active descendant, sibling
  position and set size, level, expanded state, and git status and "ignored"
  in the description. Typeahead works ("s" jumps to `src`), and it is quick.
- Closing a dirty tab asks first in a named `alertdialog` with three clear
  buttons. Focus starts on Save, and Escape and Cancel return focus to the tab.
  Unsaved text survives reload, tab close and project switch as drafts, and
  the preview says "Showing unsaved changes from the editor."
- The connection status is an honest live region, it reconnects by itself, and
  offline saves fail at once with an alert instead of hanging.
- The tab strip follows the tabs pattern: one Tab stop, arrows and Home, Delete
  to close, a named tab panel, and "unsaved changes" in the tab's name.
- Patch rows say "added" or "removed" in words, forced colours keep a marker,
  and file headers are expandable buttons with counts. The "Go to file" select
  and "Collapse all" make big diffs workable.
- Contrast is much better across the board: the shared grey is now 5.3-6.5:1.
- The start screen has a visible label, a hint, a linked error and paths as
  text. The palette announces its result count. Forced colours keep the
  palette selection.
- Layout still holds at 200% zoom of a 1024 px window: no horizontal scroll,
  and the close prompt wraps and fits.
