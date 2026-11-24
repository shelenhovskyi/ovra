# Keyboard power user (Oleh): Round 1 (2026-11-24)

Build: dd5ffce on main plus the UX harness, headless, Chromium 158.0.8127.36, 1280x800, Linux (Mod = Ctrl).
Scope: shell layout and status bar, start screen / project list, virtualized file tree, command palette, global shortcuts. Fixtures: `small`, `monorepo` (with and without seeding).

## Summary

Oleh can open a project and walk the tree without touching the mouse, and the
parts that exist behave well: tree arrows, Home/End and Left-to-parent are
correct, and the palette restores focus properly. The flow breaks down
around them, though. There is no Mod+P and the palette holds one command
(itself), so reaching a deep file in the monorepo takes 103 keystrokes. After
opening or adding a project, focus drops to `<body>`, so the first keys
silently do nothing. Overall the app feels like a solid keyboard skeleton with
nothing yet to drive.

## Findings

### KEY-1 major: No way to jump to a file; Mod+P does nothing and falls through to the browser

- **Area:** ui/src/palette (file mode), ui/src/keys (binding); needs a file-search call from the core (`fs.*`). If that API is missing, it is a protocol task first.
- **Steps:** 1. Seed `monorepo`, load the app. 2. Tab, Enter to open it, Tab into the tree. 3. Try Ctrl+P, or Ctrl+Shift+P and type `module-7`. 4. Fall back to the tree and open `packages/pkg-042/src/module-7.ts` with arrows only.
- **Expected:** Ctrl+P opens a fuzzy file finder. Typing `p42m7` and pressing Enter opens the file in about 6 keystrokes.
- **Actual:** Ctrl+P is not handled. A window listener saw `C-p defaultPrevented=false`, so a normal browser opens its print dialog, and the Tauri webview may do the same. The palette finds nothing for `file` or `project` ("No matching commands"). The tree route took **103 keystrokes** and about 5 s (s5 `01-module7.png`). Most of those were ArrowDown, because the tree has no typeahead (KEY-3).
- **Why it matters for Oleh:** "jump to any file, fast" is his main goal. Without Mod+P the persona cannot work at speed in any real repository.
- **Suggested fix:** Register a `Mod+P` command that opens the palette in file mode, backed by a project-wide file search in the core. Until that lands, have `Mod+P` at least preventDefault so it cannot trigger printing.

### KEY-2 major: Focus is lost after opening or adding a project, and keys silently do nothing

- **Area:** ui/src/tree (Tree.svelte / FileTree.svelte / ProjectList.svelte)
- **Steps:** 1. Start screen. Tab to a project button and press Enter (or Space), or type a path in "Project folder" and press Enter. 2. Press ArrowDown.
- **Expected:** The file tree has focus, and ArrowDown selects the first row.
- **Actual:** `document.activeElement` is `<body>` after both open and add. ArrowDown does nothing (`arrowdown after add: body ... selected=`), and an extra Tab is needed to reach the tree (s1 `04-after-add.png`). The palette has the same pattern on the start screen: Ctrl+Shift+P, then Esc, returns focus to `<body>`. The next Tab then lands past the end of the page (`Tab: focus=body`), and the user has to press Tab twice to get back to the project list.
- **Why it matters for Oleh:** he expects predictable focus after every action. A key that silently does nothing right after the most common action is the first thing he notices.
- **Suggested fix:** When the tree mounts because the user opened a project, focus the tree viewport and select the first row (or the `openPath` row). When the palette closes and the previous element was `<body>` or no longer exists, focus the main panel's default target (the start screen's first control, or the tree) instead of `<body>`.

### KEY-3 minor: The tree has no typeahead, and its sort order makes scanning slower

- **Area:** ui/src/tree (typeahead); crates/fs-index `listing.rs` (sort order)
- **Steps:** 1. Open `small`, Tab into the tree, select `src`. 2. Type `r`, then `c`. 3. In `monorepo`, expand `packages/pkg-042/src`.
- **Expected:** Typing `r` jumps to `README.md`, and further letters refine the match (as in VS Code, netrw and most file explorers). Names sort naturally, ignoring case: `module-2` before `module-10`, `a file with spaces.txt` before `Cargo.toml`.
- **Actual:** Letters do nothing, and the selection stays on `src`. Names sort by bytes: `module-14, …, module-19, module-2, module-20 …` and `Cargo.toml, README.md, a file with spaces.txt, build.log` (s2 log, s5 `02-palette-over-tree.png`). The real core's `listing.rs` uses `a.name.cmp(&b.name)` too, so this is not just the stand-in.
- **Why it matters for Oleh:** typeahead is his fallback when Mod+P is missing (KEY-1). Byte-order sort breaks both his visual scan and his ArrowDown counting.
- **Suggested fix:** Add typeahead to the tree: printable keys build a prefix that resets after about 500 ms, and the selection moves to the next visible row starting with that prefix (case-insensitive). Separately, sort names naturally and case-insensitively in the core.

### KEY-4 minor: A focused tree with nothing selected shows no focus indicator

- **Area:** ui/src/tree (FileTree.svelte)
- **Steps:** 1. Open a project. 2. Tab into the tree.
- **Expected:** A visible focus ring, or a pre-selected row with the ring.
- **Actual:** The viewport has `outline: none`, and the ring is drawn only on `.row.selected`. With no selection, nothing on screen changes when the tree gains focus (s2 `02-tree-focused-noselection.png`). The first ArrowDown then selects `.git`, which is dimmed and ignored.
- **Why it matters for Oleh:** he cannot tell where focus went. This is the exact state he lands in after KEY-2's extra Tab.
- **Suggested fix:** On focus, if nothing is selected, select `openPath` or the first row, or draw a ring on the viewport itself under `:focus-visible`.

### KEY-5 minor: PageUp/PageDown scroll the tree but do not move the selection

- **Area:** ui/src/tree (FileTree.svelte `onkeydown`)
- **Steps:** 1. In `monorepo`, expand `generated` (5,005 rows). 2. Press End, then PageUp, PageDown, Home, ArrowDown.
- **Expected:** PageUp/PageDown move the selection one page and keep it in view. Home shows the first row.
- **Actual:** PageUp/PageDown only smooth-scroll the viewport, and the selected row leaves the view. The next arrow key jumps back. Home pressed right after PageDown selected row 0 but left `scrollTop=352` with the selection off-screen: the native smooth scroll was still running and overrode `select()`'s scroll (s4 log).
- **Why it matters for Oleh:** paging is how he crosses a 5,000-file folder without holding a key down.
- **Suggested fix:** Handle `PageUp`/`PageDown` in `onkeydown`: move the selection by `floor(clientHeight / ROW_HEIGHT)` rows and call `preventDefault()`.

### KEY-6 minor: The palette has nothing to run, and there are no keyboard routes between panels or projects

- **Area:** ui/src/keys, ui/src/palette (commands); ui/src/tree and ui/src/shell for the commands they would own
- **Steps:** 1. Press Ctrl+Shift+P anywhere. 2. Look at the list, and press Enter on the only item. 3. Type `project` or `file`. 4. With a project open, try to get back to the start screen or to another project.
- **Expected:** The palette lists at least the commands for what exists today: focus file tree, focus main area, switch/close project, reveal open file. Every command shows its shortcut.
- **Actual:** There is one entry, "Show command palette Ctrl+Shift+P". Enter on it re-runs itself, so the palette stays open with no visible change. `project` and `file` find nothing. No key or command leaves a project or switches to another: the start screen is reachable only by reloading. Fuzzy ranking and arrow wrap cannot be judged with a single item. The palette code wraps, but only one item exists to show it.
- **Why it matters for Oleh:** he moves between panels with commands, not Tab. Today the only global shortcut opens an empty list.
- **Suggested fix:** Each area registers its commands through `registerCommand` (`tree.focus`, `tree.revealOpenFile`, `projects.switch` / `projects.close`, `shell.focusMain`). Hide `palette.show` from its own list, or make it a no-op that does not count as a command.

### KEY-7 minor: Tab order dead-ends: the tree is the last stop, and the main area cannot be reached

- **Area:** ui/src/shell (Shell.svelte)
- **Steps:** 1. With a project open, focus the tree. 2. Press Tab, then Shift+Tab.
- **Expected:** Tab moves tree → main area → status bar (or something similar), and Shift+Tab reverses it.
- **Actual:** Both Tab and Shift+Tab from the tree go to `<body>`, that is, out of the app (s2: `Shift+Tab out of tree: focus=body`, `Tab from tree forward: focus=body`). Part of this comes from the Viewer and Editor placeholders not being focusable. Even so, the shell has no focus target or skip link for the main panel, and no panel-cycling key such as F6.
- **Why it matters for Oleh:** once 1.E/1.F land, he needs a reliable key to bounce between the tree and the file. That should be designed into the shell now, not left to each viewer.
- **Suggested fix:** Make `<main>` a focus target (`tabindex="-1"` plus a `shell.focusMain` command), and add F6 / Shift+F6 to cycle sidebar → main → footer.

### KEY-8 minor: The project list ignores arrow keys

- **Area:** ui/src/tree (ProjectList.svelte)
- **Steps:** 1. Seed `small` and `monorepo`. 2. Tab to the first project. 3. Press ArrowDown.
- **Expected:** Focus moves to the next project, like any list.
- **Actual:** Nothing happens (`ArrowDown on project button: button[|monorepo]`), and only Tab moves between projects. Ten projects means ten Tabs before the "Project folder" field.
- **Why it matters for Oleh:** this is a list he picks from every session.
- **Suggested fix:** Use a roving tabindex with Up/Down/Home/End over the project buttons, so the list is a single Tab stop and the folder field is one Tab away. Better still, list projects in the palette (see KEY-6).

### KEY-9 polish: Nothing on screen reveals the one shortcut that exists

- **Area:** ui/src/shell
- **Steps:** Load the app; open a project.
- **Expected:** A hint such as "Ctrl+Shift+P: commands" on the start screen or in the status bar.
- **Actual:** The only text is "Connected to ovra 0.1.0 (headless)" (s1 `01-load.png`). The palette shows its shortcut, but only once you already know how to open it.
- **Why it matters for Oleh:** he would guess Ctrl+Shift+P, but he would also guess Ctrl+P, which does nothing (KEY-1).
- **Suggested fix:** Put a short hint on the empty main area and start screen, the way VS Code's empty editor does.

### KEY-10 polish: The window title does not include the project

- **Area:** ui/src/shell
- **Steps:** Open `demo-app`, then check `document.title`.
- **Expected:** `demo-app - Ovra`, and later `file - demo-app - Ovra`.
- **Actual:** The title stays `Ovra`.
- **Why it matters for Oleh:** he switches windows with the keyboard (Alt+Tab, rofi, tmux-style launchers) by title.
- **Suggested fix:** Set `document.title` from `workspace.project` and `openPath` in the shell, and let Tauri mirror it.

## How the placeholders affect today's keyboard flows

- Enter on a file only shows its path as grey text in the Viewer placeholder. Focus stays in the tree, which is the right choice for a viewer. Nothing else can take focus, which is the source of KEY-7.
- With the store still a stub (1.B), project switching and recents cannot be judged. KEY-6 and KEY-8 are about the keyboard routes into the store, not about what it stores.

## What worked well

- Ctrl+Shift+P works from anywhere: `<body>`, the project-folder input, the tree, and the palette itself. Focus lands in the input right away, and characters typed with no delay after the shortcut are not lost (`q="show"`).
- Esc closes the palette and returns focus to the tree with the selection intact. Tab out of the palette closes it and also returns focus to the tree. Each reopen starts with an empty query, which suits a command palette.
- The palette shows its shortcut as `Ctrl+Shift+P`, highlights matched characters and states "No matching commands" clearly. Enter on an empty result does nothing harmful.
- The tree follows WAI-ARIA tree conventions: Right expands, then moves into the first child; Left moves to the parent, then collapses; Home and End work; Enter toggles folders and opens files. The selection stays in view in a 5,005-row listing with 42–50 DOM rows. Sixty ArrowDown presses took 152 ms and landed exactly 60 rows down.
- A bad project path keeps focus and text in the input and shows the error in a `role="alert"` (`not_found: no such folder: …`). Ctrl+A and retyping fixes it.
- No browser or OS shortcut is taken over: Ctrl+W, T, L, N, K, B, E and O all pass through untouched.
