# Iryna (accessibility user): Round 2 (2026-11-24)

Build: 485e5f2 on main plus f427218 (fixture additions), headless, real core,
Chromium 158.0.8127.36. Viewports: 1280x800, and 640x400 and 512x384 CSS px,
which match 200% zoom of a 1280 px and a 1024 px window. Forced colours on and
off. Scope: viewers (markdown, image, patch, plain text, binary and too-large
messages), the editor with tabs (tablist, CodeMirror, the view/edit toggle,
save and dirty state), projects (start screen, switching). Fixtures: `small`
(seeded), plus a `docs/pics.md` written by the script with one image that has
alt text and one that has none, a table, a quote and three kinds of links.

## Summary

Iryna can read files: the markdown viewer produces real headings, lists,
tables and links, image alt text comes through, and the binary and too-large
messages are plain readable text. Editing is where she gets hurt. Closing a
tab with unsaved changes, or reloading the window, gives no warning of any
kind, accessible or not. A reload loses the text for good, and a reload is
also the only way to switch projects. Save, the view/edit toggle, tab switching
and closing a tab all drop keyboard focus to `<body>`, and nothing on the page
announces "saved", "loading" or "disconnected". None of the round 1 blockers
or majors in her area are fixed. Only one changed: the palette's matched
letters now show in forced-colours mode.

## Special focus: unsaved changes

Measured in scripts s3 and s7:

| Action with unsaved text in `src/lib.rs` | Warning or dialog        | Focus afterwards | Announced | Text on disk | Text in the app                                                                               |
| ---------------------------------------- | ------------------------ | ---------------- | --------- | ------------ | --------------------------------------------------------------------------------------------- |
| Click the tab's close button (● / ×)     | none                     | `<body>`         | nothing   | unchanged    | kept in memory, but nothing shows it; reopening the file brings it back, still marked unsaved |
| Ctrl+W                                   | none                     | `<body>`         | nothing   | unchanged    | same as above                                                                                 |
| Reload the window (`page.reload()`)      | none (no `beforeunload`) | start screen     | nothing   | unchanged    | **lost**: reopening shows the file as on disk, with no unsaved marker                         |
| Save while the core connection is closed | none                     | stays            | nothing   | unchanged    | Save "succeeds" silently: the call queues forever, the ● stays, no error after 2.5 s          |

The coordinator's report holds for reload. For closing a tab the text is not
lost at once, but it is hidden: once the tab is gone, nothing tells Iryna that
a closed file still holds unsaved text. That text is then lost on the next
reload or project switch. Because there is no warning, the question of whether
a warning is accessible does not come up yet. No code path in `ui/src`, `app/`
or `crates/` guards against it (`beforeunload`, `onCloseRequested`).

## Findings

### A11Y-13 blocker: unsaved text is lost on reload and hidden on tab close, with no warning

- **Area:** ui/src/editor (Tabs, session, commands), ui/src/shell for the window-level guard, app/ for the Tauri close request
- **Steps:** 1. Open `src/lib.rs`, press Ctrl+E, type a line. 2. Click "Close lib.rs (unsaved changes)", or press Ctrl+W. 3. Open the file again, type more, then reload the window.
- **Expected:** Closing a dirty tab opens a dialog: "Save changes to lib.rs?", with Save, Don't save and Cancel. It is a labelled `role="alertdialog"`, focus starts on Save, and Escape cancels and returns focus to the tab. Reloading or closing the window while files are dirty raises the browser's `beforeunload` prompt in headless mode, and Tauri's close-requested handler does the same in the app.
- **Actual:** No dialog for either action (`dialogs=[]`). Focus falls to `<body>`, and no live region announces anything. The closed file stays dirty in memory with no tab and no other marker. A reload drops all unsaved text silently: the file reopens as it is on disk (s7b: `editor after reload="[package]name = \"demo\"version = \"0.1.0\""`, with `UNSAVED-LINE` gone). There is no in-app way to switch projects, so a reload is also how she switches projects, and it loses everything unsaved.
- **Why it matters for Iryna:** she works by ear and cannot glance at the tab strip to check for dots. A key press that silently loses work, and announces nothing, is the worst case for her.
- **Suggested fix:** in the editor area, add a "confirm close" step for dirty tabs, used by the close button and Ctrl+W, built as an accessible alertdialog. In the shell, add `beforeunload` while `dirtyFiles` is not empty. In `app/`, handle the window's close request. Unit-test "closing a dirty tab asks first", and add one e2e test for the reload prompt.

### A11Y-14 major: save, toggle, tab switch and close all drop focus to `<body>`

- **Area:** ui/src/editor (Editor, Tabs, commands), ui/src/viewers
- **Steps:** 1. In the editor, Tab to "Save" and press Enter. 2. In the editor, press Ctrl+Tab, or Ctrl+E to go back to view mode. 3. Press Ctrl+W, or Enter on a tab's close button. 4. In a markdown file, press Enter on the link `guide`.
- **Expected:** Focus lands somewhere sensible and is announced: the editor stays focused after a save, the new tab's editor or viewer after a switch, the next tab after a close, and the new document after following a link.
- **Actual:** In every case `document.activeElement` is `<body>` (s2, s5, s8). "Save" disables itself once the text is saved, and a disabled button loses focus. The editor is destroyed and recreated on every switch, so its focus is never restored. Entering edit mode from the tree leaves focus in the tree, so the cursor is not in the editor either.
- **Why it matters for Iryna:** after every one of these steps her screen reader goes silent, and with RSI she spends extra key presses finding her place again.
- **Suggested fix:** keep "Save" enabled, as a no-op when clean (or use `aria-disabled`), or move focus back to the editor after saving. After a tab switch or toggle, focus the new editor (`view.focus()`) or the viewer's container (`tabindex="-1"`). After a close, focus the newly active tab. After following a link, focus the new document.

### A11Y-15 major: save, load and error states are not announced; dirty state is only partly spoken

- **Area:** ui/src/editor, ui/src/viewers
- **Steps:** 1. Edit a file, press Ctrl+S. 2. Open `data/huge.txt` and `data/blob.bin`. 3. Delete an open file on disk.
- **Expected:** "Saved lib.rs" is announced through a polite `role="status"`. The "Loading…", "binary", "too large" and "no longer exists" messages are in a live region, or focus moves to them. The unsaved state is part of the tab's own name ("lib.rs, unsaved") and of the editor's label.
- **Actual:** The only live regions on the page are CodeMirror's own `cm-announced` and the conflict `role="alert"`. A successful save changes only the ● and the disabled state of "Save", and neither is announced. Viewer messages are plain `<p>`. Dirty state is spoken only on the close button ("Close lib.rs (unsaved changes)"). The tab itself reads "lib.rs", and the editor bar's ● has only a `title`, so screen readers read "black circle" or nothing.
- **Why it matters for Iryna:** she cannot tell whether Ctrl+S worked, or whether a file she opened is still loading or failed.
- **Suggested fix:** use one visually hidden `role="status"` in the editor area for "Saved", "Save failed" and "Loading", and `role="alert"` for viewer errors. Add ", unsaved" to the tab's accessible name. Give the ● `aria-hidden` and put the state in text.

### A11Y-16 major: the tab strip does not follow the WAI-ARIA tabs pattern

- **Area:** ui/src/editor (Tabs.svelte)
- **Steps:** 1. Open three files. 2. Focus a tab and press ArrowRight. 3. Tab through the strip. 4. Read the accessibility tree.
- **Expected:** One Tab stop for the strip. Arrow keys move between tabs, and Delete (or the Ctrl+W command) closes one. Every tab has `aria-controls` pointing at a `role="tabpanel"` that holds the editor or viewer. The tablist owns only tabs.
- **Actual:** ArrowRight does nothing. Every tab and every close button is its own Tab stop, so 10 files make 20 stops. The close buttons are children of the `tablist`, which allows only tabs, so screen readers count them oddly. There is no `tabpanel` and no `aria-controls`. In forced-colours mode the active tab looks the same as the others (`fc-patch-view.png`). In normal mode the only cue is a text colour change (6.7:1 to 14.5:1), because the background differs by only 1.03:1. At 512 px width (200% zoom), Ctrl+Shift+Tab can make a tab active while it stays scrolled out of view (`tab x=821..964 list=260..512`). Close buttons are 20x20 px, below the 24x24 px target size.
- **Why it matters for Iryna:** she cannot move through her open files the way every other tabbed app lets her, and at high contrast she cannot see which file is active.
- **Suggested fix:** use roving `tabindex` on the tabs, with arrows, Home and End, and Delete to close. Keep the close buttons `tabindex="-1"`, outside the tablist's ownership (`aria-owns`), or reachable only by Delete. Add a tabpanel around the main view. Give the active tab a 2 px top or bottom border that forced colours keep (use `Highlight` under `forced-colors`). Call `scrollIntoView({ inline: "nearest" })` on the active tab. Make the close buttons 24 px.

### A11Y-17 major: patch view conveys added and removed lines by colour and a bare +/- only

- **Area:** ui/src/viewers (PatchView.svelte)
- **Steps:** 1. Open `changes.patch`. 2. Read the table with a screen reader, then turn on forced colours.
- **Expected:** Each changed line is spoken as "removed line 2: a + b" or "added line 2: a.wrapping_add(b)". Columns have headers ("old", "new", "change"). In forced colours, added and removed lines still differ by something other than the `+`/`-` character.
- **Actual:** Rows read `"2 - a + b"` and `"2 + a.wrapping_add(b)"`. The leading `-` or `+` sits in the same text as the code, so at the default punctuation level a screen reader skips it or reads it as part of the expression ("a plus b"). The table has no header row and no caption, and the hunk header occupies the line-number column. In forced colours, the red and green backgrounds go away (`fc-patch-view.png`). Only the one-character prefix is left, with no border or marker. The "new" and "deleted" badges are styled by class, but the `.added` and `.removed` colours are defined for rows only. Line numbers are 3.64:1.
- **Why it matters for Iryna:** reviewing agent diffs is a core task, and today she has to infer by ear which lines were removed.
- **Suggested fix:** put the prefix in its own cell, with a visually hidden "added", "removed" or "unchanged" label (or render the code with `<ins>` and `<del>`). Add `<thead>` with visually hidden column headers. In forced colours, use a left border (`border-left: 3px solid` on the row with `CanvasText`) or `forced-color-adjust` with system colours. Raise the line-number colour to the shared muted token (at least #8a8a96).

### A11Y-18 minor: the view/edit toggle is invisible and unnamed; the editor has no accessible name

- **Area:** ui/src/editor
- **Steps:** 1. Open `Cargo.toml`. 2. Look for a way to edit it. 3. Press Ctrl+E and read the accessibility tree.
- **Expected:** A visible "Edit" or "View" toggle button, with `aria-pressed` or a clear label, and a mode change announced ("Editing Cargo.toml"). The CodeMirror `textbox` is named after the file.
- **Actual:** The only way in is Ctrl+E, or the palette entry "Toggle editing the open file". Nothing on screen or in the accessibility tree says which mode is active. In view mode there is no file name either, except the selected tab. The editor is an unnamed `textbox` (`role=textbox aria-multiline=true`, no label). Tab inside the editor inserts an indent, which is correct, but nothing tells her that Escape then Tab leaves. That sequence then lands on `<body>`, because nothing after the editor can take focus. In view mode, the image figure's accessible name is its size caption ("1 × 1"), and its alt text is the raw path (`assets/logo.png`).
- **Suggested fix:** add a View/Edit toggle button in a toolbar above the main area, labelled with the mode, plus a heading or `aria-label` naming the open file on the viewer's container. Pass `EditorView.contentAttributes.of({ "aria-label": path })`, and mention "Escape then Tab to leave the editor" in its `aria-describedby`. Label the image figure with the file name.

### A11Y-19 minor: viewer and editor greys below 4.5:1

- **Area:** ui/src/viewers, ui/src/editor (theme.ts)
- **Steps:** 1. Compute contrast from computed styles (s4).
- **Actual:** Editor gutter line numbers #5a5a66 on #16161a measure 2.65:1. Viewer notes ("Loading…", "Binary file … cannot be shown.", "This file is too large to show.") and patch line numbers use #6f6f7a, which measures 3.64:1. That is the same grey as A11Y-7 in round 1. The comment token #6a737d is about 3.9:1. The focus ring on tabs is Chromium's default `auto` (1 px, #101010 outer) on a near-black background, so only its thin inner light ring shows. Good: tab text measures 6.7 and 14.5:1, patch code 11.4 and 12.5:1, hunk headers 5.3:1, markdown code 13.4:1.
- **Suggested fix:** use one shared muted token of at least #8a8a96 in the viewer, the patch view and the editor gutter. Comments should be at least #8b949e. Give `button:focus-visible` an explicit 2 px `#4c6ef5` outline across the shell.

### A11Y-20 minor: markdown details

- **Area:** ui/src/viewers (MarkdownView, markdown.ts)
- **Steps:** 1. Open `README.md` and `docs/pics.md`.
- **Actual:** Most of it is good (see below). Task-list checkboxes are unnamed, so they are read as "checkbox, not checked, unavailable", followed by the text as a separate node. Activating an anchor link (`#table`) scrolls the page, but focus stays on the link, and so does the reading position. The rendered document has no accessible name (`article` with no label).
- **Suggested fix:** wrap the task item text in a `<label>`, or set `aria-label` from the item text. After an in-page anchor jump, focus the target heading (`tabindex="-1"`). Use `aria-label` = file name on the article.

## Round 1 re-check

| ID     | Round 1 severity | Status     | Evidence (round 2)                                                                                                                                                                                                                                                                                                      |
| ------ | ---------------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A11Y-1 | blocker          | still open | Two ArrowDown presses on the tree leave focus on `DIV role=tree ad=null`, so nothing is announced. A mouse click now moves DOM focus to the row (`tabindex=-1`), but that does not help keyboard use.                                                                                                                   |
| A11Y-2 | major            | still open | Names are still `"▸ docs •"` and `"▸ src •"`, and posinset/setsize still count the flattened list (`FileTree.svelte` unchanged).                                                                                                                                                                                        |
| A11Y-3 | major            | still open | Directory status is still a coloured "•", ignored entries still use `opacity: 0.5`, and in forced colours every dot looks the same (`fc-patch-view.png`, sidebar).                                                                                                                                                      |
| A11Y-4 | major            | still open | After the socket is closed (s7b), the status still reads "Connected to ovra 0.1.0 (headless)". Calls made after the close are queued forever, so Ctrl+S does nothing and shows no error. The footer is still not a live region. Pending calls are now rejected on close (`transport.ts:97`), which is a partial change. |
| A11Y-5 | major            | still open | After opening a project from the start screen with Enter, focus is on `BODY` (s6).                                                                                                                                                                                                                                      |
| A11Y-6 | major            | changed    | Matched letters now show in bold in forced colours (`fc-palette.png`). The selected option still has no visible indicator, so it is still unclear which command Enter runs. Remaining severity: major.                                                                                                                  |

Round 1 minors, briefly: A11Y-7 (muted grey 3.64:1) is still open and now
also affects viewers (A11Y-19). A11Y-10 (13 px root font, fixed 260 px
sidebar) is still open, but layout at 640 and 512 px holds, with no page-level
horizontal scroll. A11Y-11 (no palette live region) is still open: the only
live regions are CodeMirror's and the conflict alert. A11Y-12 is still open:
the start screen shows the "File tree" heading followed by "Projects". The
summary's issue 8 (reload forgets the open project, and there is no way to
switch projects) is still open, and it now loses unsaved text (A11Y-13). Open
tabs are remembered per project.

## What worked well

- The markdown viewer renders semantic HTML: h1 and h2 headings with ids, real
  lists, a table with `columnheader` cells, blockquote, and links that work with
  Enter. Raw `<script>` is shown as escaped text.
- Image alt text from markdown passes through (`img "Ovra logo"`), and an empty
  alt stays empty, so the image is treated as decorative.
- Binary and too-large files get clear plain-language messages ("Binary file
  (application/octet-stream) cannot be shown.", "This file is too large to
  show.") instead of garbage or a hang.
- The conflict on save is a `role="alert"` with two clearly named buttons,
  "Overwrite" and "Reload from disk", and focus stays in the editor.
- Close buttons have good labels, including "(unsaved changes)", and the
  dirty ● is a shape, not a colour, so it survives forced colours.
- CodeMirror exposes `role=textbox aria-multiline=true` and its own polite live
  region. All tab, close, next and previous commands are in the palette with
  their shortcuts.
- Patch code contrast is high (11-12:1), and nothing overflows the page at 200%
  zoom.
