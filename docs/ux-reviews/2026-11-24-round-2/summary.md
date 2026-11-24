# Round 2 summary (2026-11-24)

Build: `f427218` on `main` (485e5f2 plus fixture additions), headless, real
core (debug build), Chromium, 1280x800. No stand-ins.
Scope: viewers, the editor with tabs, projects and per-project state,
open-external; special focus on unsaved changes. Re-check of round 1.

Reports: [newcomer](newcomer.md), [keyboard-power-user](keyboard-power-user.md),
[monorepo-maintainer](monorepo-maintainer.md),
[agent-orchestrator](agent-orchestrator.md),
[accessibility-user](accessibility-user.md). New findings are merged into the
issues below, ranked, by owning area.

## What works

Markdown, images, binary and too-large messages are clear; markdown keeps its
headings, tables, links and alt text for screen readers. Viewers refresh
72-147 ms after an outside write and keep their scroll position. The editor
handles a 20,000-line file (126 ms to highlight a new viewport, typing keeps
up). The save conflict banner is a proper `role="alert"` with named buttons.
Reopening a project restores its tabs, active file and expanded folders.

## Unsaved changes (all five personas)

Confirmed: unsaved text is lost, and nothing warns. Five separate paths:

1. **Reload or window close** loses every unsaved edit: no `beforeunload`
   handler (and no Tauri close guard); unsaved text lives only in a module-level
   map in `ui/src/editor/session.svelte.ts`. After the reload the restored tabs
   look clean and show the disk text. A reload is also today's only way to
   switch projects, and the only recovery when the core connection drops.
   (NEW-11, KEY-11, MONO-11, ORCH-11, A11Y-13)
2. **Closing a dirty tab** (× or Ctrl+W) asks nothing; the text stays hidden in
   memory until the next reload. An empty editor bar with ● and a dead Save
   button stays behind. (NEW-12, KEY-12, ORCH-14)
3. **The 20-tab limit** silently closes the oldest tab, dirty or not.
   (NEW-14, MONO-12)
4. **An outside write to a dirty background tab** (agent, formatter,
   checkout) replaces the edits with the disk text, clears ●, and undo cannot
   bring them back: `load()` drops the unsaved copy when mtime differs.
   (NEW-13, ORCH-10)
5. **Save overwrites or recreates without a conflict**: undo past an outside
   reload, then Save, writes old text over the agent's version; saving a file
   deleted or renamed on disk recreates it, because `crates/fs-index`
   `files::write` skips the `expected_mtime_ms` check for missing files.
   "Reload from disk" in the conflict banner discards edits without asking.
   (ORCH-12, ORCH-13, NEW-16, ORCH-15)

Also: in headless mode in a real browser Ctrl+W, Ctrl+Tab and Ctrl+Shift+Tab
belong to the browser, so the advertised "Close tab" (Ctrl+W) closes the whole
Ovra page, which with path 1 is one-key data loss (KEY-13, NEW-14; known
Chromium behaviour, Playwright cannot show it). Ctrl+S while disconnected
fails silently (A11Y-13, ORCH-11).

## Issues by area

| #   | Severity                      | Issue                                                                                                                                                                                                                      | Area                                             | Findings                                |
| --- | ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ | --------------------------------------- |
| 1   | blocker                       | Unsaved text lost: reload/close guard, dirty-tab close prompt, tab limit, outside writes to dirty tabs (paths 1-4 above)                                                                                                   | `ui/src/editor` (+ `app/` close guard)           | see above                               |
| 2   | blocker                       | Save writes over outside changes or recreates deleted files (path 5)                                                                                                                                                       | `ui/src/editor`, `crates/fs-index`               | ORCH-12, ORCH-13, NEW-16                |
| 3   | blocker (round 1, still open) | Lost core connection invisible; calls, including Save, hang forever                                                                                                                                                        | `ui/src/lib/transport.ts`, shell                 | ORCH-1, A11Y-4                          |
| 4   | blocker (round 1, still open) | Screen readers hear nothing in the tree                                                                                                                                                                                    | `ui/src/tree`                                    | A11Y-1                                  |
| 5   | major                         | Browser-reserved shortcuts in headless mode (Ctrl+W, Ctrl+Tab, Ctrl+P); owner should choose browser-safe defaults                                                                                                          | `ui/src/keys`, `ui/src/editor`                   | KEY-13, KEY-1                           |
| 6   | major                         | Focus drops to `<body>` after Save, Ctrl+E, Ctrl+Tab, closing a tab, following a link, opening a project; Ctrl+E leaves focus in the tree (2n+1 Tabs to reach the text); nothing announces saved/loading; ● is visual only | `ui/src/editor`, `ui/src/viewers`, `ui/src/tree` | KEY-14, KEY-15, A11Y-14, A11Y-15, KEY-2 |
| 7   | major                         | Undo history is lost on tab switch or view/edit toggle; editor jumps to line 1 on every outside write                                                                                                                      | `ui/src/editor`                                  | KEY-16, ORCH-17                         |
| 8   | major                         | Every save or git change re-lists every expanded folder: 403 requests and a 1,057 ms freeze with 402 folders open; reopening such a project blocks ~1.4 s                                                                  | `ui/src/tree`                                    | MONO-1, MONO-14, ORCH-6                 |
| 9   | major                         | Big diffs and big text render in full: 30,000-line diff = 120,095 DOM nodes, 1,481 ms freeze; 4.8 MB text 526 ms; no per-file list or collapse in patches                                                                  | `ui/src/viewers`                                 | MONO-15                                 |
| 10  | major                         | No project switch, close or remove; a reload lands on the start screen                                                                                                                                                     | `ui/src/shell`, `ui/src/tree`                    | NEW-17, MONO-17, ORCH-4, KEY-6          |
| 11  | major                         | Tabs: no arrow keys or tabpanel, two Tab stops per file, active tab invisible in forced colours and can scroll out of view, 20 tabs all named `module-N.ts` with no path                                                   | `ui/src/editor` (Tabs)                           | A11Y-16, MONO-13, KEY-17                |
| 12  | major                         | No visible way to start editing (Ctrl+E only); tree click drops to view mode, tab click keeps edit mode; outside changes show nothing until Save; conflict banner has no compare                                           | `ui/src/editor`                                  | NEW-15, MONO-18, KEY-19, ORCH-15        |
| 13  | major                         | Patch view conveys added/removed by colour and a one-character prefix only                                                                                                                                                 | `ui/src/viewers`                                 | A11Y-17                                 |
| 14  | major (round 1, still open)   | Raw error codes (`bad_request:`, "(os error 2)")                                                                                                                                                                           | start screen, `ui/src/lib`                       | NEW-18, NEW-2                           |
| 15  | minor                         | `open.external` exists but no button, command or menu reaches it; too-large/binary messages give no size or "open in default app"                                                                                          | `ui/src/viewers`, `ui/src/editor`                | ORCH-16, MONO-16, NEW minor             |
| 16  | minor                         | Preview of a dirty file shows the saved version without saying so; broken links open a "no longer exists" tab; failed web link shows nothing; edit mode offered for binary files                                           | `ui/src/viewers`, `ui/src/editor`                | NEW minor, KEY-21                       |

## Round 1 re-check

Fixed: adding a project (NEW-1), file click shows a real viewer (NEW-4).
Partly fixed: per-project state (tabs, active file, expanded folders come back
when a project is reopened; the project itself, selection and scroll do not);
the palette now has editor commands but none for projects or focus; Tab now
passes through the tab strip; forced colours now show palette matches but not
the selected option (A11Y-6).
Still open: connection loss (ORCH-1, worse now that Save depends on it), tree
screen-reader support and semantics (A11Y-1 to A11Y-5), focus after opening a
project (KEY-2), Ctrl+P and typeahead (KEY-1, KEY-3), full re-list on change
(MONO-1, worse with the real core), finding changes (MONO-2), selection loss
(MONO-3, ORCH-3), deleted files in listings (ORCH-2), raw error codes (NEW-2).

## Suggested tasks

1. **Editor: never lose unsaved text** (`ui/src/editor`, small `app/` part):
   `beforeunload` and Tauri close-requested guards; Save / Don't save / Cancel
   on closing a dirty tab, and never auto-close a dirty tab at the tab limit;
   keep edits and show the conflict notice when a dirty file changes on disk;
   keep undo across tab switches. Hot exit (unsaved text persisted per
   project) later, after a protocol field. Highest priority.
2. **fs-index: conflict on missing files** (`crates/fs-index`): treat a
   missing file with `expected_mtime_ms` set as a conflict; editor offers
   "Save as new" or discard.
3. **Keys: browser-safe defaults in headless mode** (`ui/src/keys`): owner UX
   decision on alternatives to Ctrl+W, Ctrl+Tab and Ctrl+P.
4. **Focus and announcements** (editor, viewers, tree): focus the editor on
   Ctrl+E, keep focus in the tab strip or editor after close/switch, a polite
   live region for saved/loading/conflict, ● with a text equivalent.
5. **Viewers at scale**: virtualize the patch view with a file list and
   per-file collapse; use the read-only editor for big text.
6. Round 1 tasks for transport, tree focus model, incremental refresh and
   projects remain open and are now more urgent.
