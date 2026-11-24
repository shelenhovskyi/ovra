# Iryna (accessibility user): Round 1 (2026-11-24)

Build: dd5ffce on main plus the UX harness, headless, Chromium 158.0.8127.36,
1280x800, plus 640x400 and 512x384 CSS px (200% zoom of a 1280 and a 1024 px
window), forced colors, reduced motion, 200% text size and WCAG text spacing.
Scope: shell layout and status bar, start screen / project list, virtualized
file tree, command palette, global shortcuts. Fixtures: `small` (seeded).

## Summary

With a mouse and good eyesight the flow works, and the palette is a well-built
combobox, but with a screen reader Iryna cannot use the file tree: arrow keys
move a visual highlight that is never exposed to assistive technology, and
because the tree is virtualized, browse mode cannot reach rows outside the
viewport either. Status information is weak across the board. Git status and
"ignored" are given as colour, opacity or bare glyphs, the status bar is not a
live region and keeps saying "Connected" after the core goes away, and focus
is dropped to `<body>` when a project opens. Layout holds up well at 200% zoom
and with text spacing. Contrast is mostly good, but secondary and placeholder
text sits at 3.3-3.7:1, and in forced-colours mode the palette hides both the
matched letters and the selected option.

## Findings

### A11Y-1 blocker: arrowing through the file tree announces nothing to a screen reader

- **Area:** ui/src/tree
- **Steps:** 1. Open `demo-app`. 2. Tab into the tree. 3. Press ArrowDown a few times, then ArrowRight on `src`.
- **Expected:** Each key press announces the current row, for example "src, expanded, level 1, 5 of 13". The tree uses either roving `tabindex` with DOM focus on the row, or `aria-activedescendant` on the focused tree.
- **Actual:** `document.activeElement` stays on `div[role=tree]` with no `aria-activedescendant` (logged: `DIV tree ad=null`). Rows get `aria-selected` and a CSS outline but never receive focus, so NVDA/VoiceOver say "Files, tree" once and then stay silent. The tree is also virtualized, so browse-mode reading only reaches the ~45 rows in the DOM. A project bigger than one screen cannot be explored at all.
- **Why it matters for Iryna:** the tree is the only way to find and open a file today, so she cannot do the main task.
- **Suggested fix:** set `aria-activedescendant` on the tree to the current row's id (give rows `id="tree-row-<index or hash>"`) and make sure the active row is always rendered (it already is, since `select()` scrolls it into view). Alternatively, move DOM focus to the row (`tabindex=0` on the current row, `-1` on the others). Add a unit test that the active descendant follows ArrowDown/ArrowRight.

### A11Y-2 major: tree items have wrong position and set size, and junk glyphs in their names

- **Area:** ui/src/tree
- **Steps:** 1. Open `demo-app`, expand `src`. 2. Read the accessibility tree (`ariaSnapshot`).
- **Expected:** `src/util` is "util, collapsed, level 2, 1 of 4", and names hold only the file name plus a spoken status.
- **Actual:** `aria-posinset`/`aria-setsize` count the flattened visible list: `src` is 5/17 and `src/util` 6/17, although `src` has 4 children and the root has 13 entries. Accessible names include the twisty and badge glyphs: `"▸ src •"`, `"main.rs M"`, `"new_module.rs U"`, and `"▾ docs …"` while loading. Screen readers read these as "black right-pointing small triangle src bullet" and "main.rs M".
- **Why it matters for Iryna:** wrong counts mislead her about how big a folder is, and the glyph noise slows every row down.
- **Suggested fix:** compute posinset/setsize among siblings (the model knows each row's parent). Mark the twisty and badge `aria-hidden="true"`. Expose status as text, for example a visually hidden ", modified" or `aria-description="modified"`, and set `aria-busy="true"` on a loading directory instead of rendering "…" in the name.

### A11Y-3 major: git status and "ignored" are colour-only or hidden from assistive tech

- **Area:** ui/src/tree
- **Steps:** 1. Open `demo-app`. 2. Look at `src`, `node_modules`, `target`, `build.log` and `.git`. 3. Repeat with forced colours on (`fc-tree.png`).
- **Expected:** Every state has a text or shape signal and a spoken equivalent.
- **Actual:** A directory with changes gets a "•" whose colour alone says which kind of change it is: yellow means modified, green added or untracked, red deleted. Ignored entries only get `opacity: 0.5`, which is not exposed to assistive tech, and their text drops to 4.45:1 (twisty 2.26:1). The file letters M/U carry the meaning only in a `title` tooltip ("modified"), which shows on hover only. In forced colours all git colours disappear, so the dot looks the same for every kind of change.
- **Why it matters for Iryna:** she cannot tell modified from untracked folders, or ignored from tracked files, either by ear or at high contrast.
- **Suggested fix:** use a distinct letter or icon for directories (for example the most significant child status letter), add spoken text as in A11Y-2, and add "ignored" to the accessible description. Keep ignored text at 4.5:1 or more by using a lighter grey instead of opacity.

### A11Y-4 major: status bar is not a live region and keeps saying "Connected" after the core is gone

- **Area:** ui/src/shell (with ui/src/lib/transport.ts)
- **Steps:** 1. Open `demo-app`. 2. Close the WebSocket (`socket.close()` from the page, as when the daemon dies). 3. Expand `docs`.
- **Expected:** The status bar changes to "Disconnected" (or "Reconnecting…") and the change is announced. Actions that cannot finish report an error.
- **Actual:** The status still reads "Connected to ovra 0.1.0 (headless)". `docs` shows "…" forever (`disconnected.png`): new frames are queued for a socket that never reopens, and nothing is announced. Even the initial "Connecting…" → "Connected" change is silent, because the footer has no `role="status"` or `aria-live`.
- **Why it matters for Iryna:** she relies on announcements for state changes. A silent hang looks to her like her screen reader has stopped working.
- **Suggested fix:** the transport exposes connection state (open/closed) and rejects calls made while closed. The shell renders it in a `role="status"` footer, with errors in `role="alert"`. Add a unit test for "socket closes → status says disconnected".

### A11Y-5 major: opening a project drops keyboard focus to `<body>`

- **Area:** ui/src/tree (ProjectList → FileTree handover)
- **Steps:** 1. On the start screen, Tab to `demo-app` and press Enter.
- **Expected:** Focus moves to the file tree (or its first row), and the screen reader announces "demo-app opened, Files tree".
- **Actual:** The project button is removed, so focus falls to `<body>` (logged: `after open focus: <body>`). Nothing is announced, and the user has to Tab again to find the tree.
- **Why it matters for Iryna:** she loses her place on every project switch and gets no confirmation that anything happened. With RSI, every extra key press counts.
- **Suggested fix:** focus the tree once its first listing has loaded, after a project opens from the start screen.

### A11Y-6 major: in forced-colours mode the palette hides matched letters and the selected option

- **Area:** ui/src/palette
- **Steps:** 1. Turn on forced colours. 2. Press Ctrl+Shift+P and type `pal`.
- **Expected:** The option text reads in full, and the selected option has a visible indicator.
- **Actual:** The matched letters disappear ("Show command \_\_\_\_ ette", `fc-palette.png`): `<mark>` with `background: none` gets MarkText on Canvas. The selected option relies on `background: #2d3a5c` only, which forced colours removes, so nothing shows which option Enter will run.
- **Why it matters for Iryna:** high-contrast mode is one of her main tools. With more than one command, she would be choosing blind.
- **Suggested fix:** add `@media (forced-colors: active)` rules. Give `mark` `color: Highlight` or an underline with `forced-color-adjust: none` and system colours, and give the selected option `outline: 2px solid Highlight` (or `background: Highlight; color: HighlightText`).

### A11Y-7 minor: several texts below 4.5:1 contrast

- **Area:** ui/src/tree, ui/src/palette, ui/src/shell
- **Steps:** 1. Compute contrast from computed colours on each screen (script s5).
- **Expected:** 4.5:1 or more for text.
- **Actual:** placeholder `/path/to/project` 3.72:1, "Type a command" placeholder 3.60:1, "No matching commands" 3.34:1, tree "Loading…" / "Empty project" (#6f6f7a) 3.64:1, ignored entries 4.45:1, shortcut `kbd` on the selected palette row 4.03:1 at 11 px. The tree's selected-but-unfocused row differs from the background by only 1.32:1, and the hover state by 1.10:1.
- **Why it matters for Iryna:** with low vision she misses hints and cannot see which file is open once focus leaves the tree.
- **Suggested fix:** raise the muted grey to at least #8a8a96 (5.3:1 on #16161a), and reuse one muted token everywhere. Give the selected row a 3:1 marker, such as a left bar or a 1 px border, in addition to the fill.

### A11Y-8 minor: tree gives no focus indicator until an arrow key is pressed

- **Area:** ui/src/tree
- **Steps:** 1. Open a project. 2. Tab into the tree.
- **Expected:** A visible focus ring on the tree or on its first or current row.
- **Actual:** The viewport has `outline: none` and no row is current yet, so nothing shows that the tree has focus (`tree-focused-initial.png`). Once a row is current, its ring is a 1 px #4c6ef5 line: 3.16:1 against the selection fill, which passes 3:1 but is thin.
- **Suggested fix:** on focus, make the first row (or the open file's row) current. Use a 2 px ring.

### A11Y-9 minor: add-project field has no visible label and a loose error

- **Area:** ui/src/tree (ProjectList)
- **Steps:** 1. Type `/does/not/exist` in the folder field and press Enter.
- **Expected:** A visible label, an error tied to the field, and a human message.
- **Actual:** The only visible cue is a low-contrast placeholder, which vanishes once she types. The error "not_found: no such folder: /does/not/exist" is announced via `role="alert"`, but the input gets no `aria-invalid` or `aria-describedby`, and the message starts with a raw error code. Each project's full path is only in a `title` tooltip, which shows on hover only.
- **Suggested fix:** add a visible `<label>`, and add `aria-invalid` plus `aria-describedby` pointing at the error. Strip the error code from the message. Show the project path as secondary text under the name.

### A11Y-10 minor: fixed pixel sizes ignore the user's text size; sidebar width fixed

- **Area:** ui/src/shell, ui/src/tree
- **Steps:** 1. Raise the root/body font size to 200% (26 px), as a user's default font setting would (`text-200.png`). 2. Use 200% zoom on a 1024 px window (`zoom200-1024-tree.png`).
- **Expected:** Text follows the user's preferred size, rows grow with it, and long names stay readable.
- **Actual:** `html, body { font-size: 13px }` overrides the browser or OS default font size. Tree rows are a fixed 22 px, so 26 px text overlaps the rows next to it. At 200% zoom of a 1024 px window, the 260 px sidebar takes half the width and cannot be resized, and long names are cut with "…" with no way to read them by sight. Zoom itself works: no horizontal scroll and nothing overlaps.
- **Suggested fix:** use rem/em sizes off the user default, and derive `ROW_HEIGHT` from the computed line height. Add a resizable sidebar (or collapse it below ~700 px), and give truncated rows a `title`.

### A11Y-11 minor: palette gives no spoken result count or no-match feedback

- **Area:** ui/src/palette
- **Steps:** 1. Ctrl+Shift+P, type `zzz`.
- **Expected:** "No matching commands" (or "3 commands") is announced.
- **Actual:** No live region exists on the page (logged `live regions:` empty). The empty state is a `presentation` item inside the listbox, so screen readers say nothing while filtering. The combobox also lacks `aria-autocomplete="list"`.
- **Suggested fix:** add a visually hidden `role="status"` with the result count, and set `aria-autocomplete="list"`.

### A11Y-12 polish: landmark and heading structure is a bit off

- **Area:** ui/src/tree, ui/src/shell
- **Steps:** 1. Read the accessibility tree of the start screen.
- **Actual:** The complementary landmark holds the heading "File tree" directly followed by the heading "Projects", even when no tree is shown. The sidebar landmark has no name, and the main area has no heading. Shortcuts are only discoverable inside the palette: there is no `aria-keyshortcuts`, and there is no key to move between sidebar and main (F6 or similar).
- **Suggested fix:** render the hidden "File tree" heading only with the tree, label the `aside` ("Sidebar"), give `main` a heading once viewers land, and add region-cycling keys in `ui/src/keys`.

## What worked well

- The command palette follows the WAI-ARIA combobox pattern correctly: `role="combobox"`, `aria-controls`, `aria-activedescendant` and a labelled `dialog`. Escape returns focus to where it came from (the tree), and Ctrl+Shift+P also works from inside a text field.
- Landmarks exist (banner, complementary, main, contentinfo), there is a page `h1`, and project-list errors use `role="alert"`.
- Main text contrast is strong: body text 14.5:1, muted labels 6.5:1, error red 6.5:1, git colours 5-10:1.
- At 200% zoom of 1280 and 1024 px windows, nothing scrolls horizontally or overlaps, and WCAG text-spacing overrides break nothing.
- There are no animations or transitions, so reduced motion is respected by default.
- Native buttons and inputs keep the default visible focus ring.
- Tree keyboard keys (arrows, Home/End, Enter, Left to parent) match the WAI-ARIA tree pattern. Only their exposure to assistive tech is missing (A11Y-1).
