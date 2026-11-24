# Round 3 summary (2026-11-24): regression

Build: `9512ea6` on `main`, headless, real core (debug build), Chromium.
Scope: every finding from rounds 1 and 2, plus what landed since: unsaved-text
protection, project switcher, transport disconnect handling, browser-safe
shortcuts (Alt+W, Alt+], Alt+[, Alt+P), Go to file over `fs.list_files`,
virtualized diffs, visible-rows tree refresh.

Reports: [newcomer](newcomer.md), [keyboard-power-user](keyboard-power-user.md),
[monorepo-maintainer](monorepo-maintainer.md),
[agent-orchestrator](agent-orchestrator.md),
[accessibility-user](accessibility-user.md).

## Regression result

| Persona             | Re-checked | Fixed  | Partly | Still open | Regressed | New    |
| ------------------- | ---------- | ------ | ------ | ---------- | --------- | ------ |
| newcomer            | 22         | 15     | 6      | 1          | 0         | 7      |
| keyboard-power-user | 21         | 12     | 6      | 3          | 0         | 7      |
| monorepo-maintainer | 18         | 8      | 9      | 1          | 0         | 6      |
| agent-orchestrator  | 20         | 9      | 10     | 1          | 0         | 8      |
| accessibility-user  | 20         | 8      | 9      | 3          | 0         | 5      |
| **Total**           | **101**    | **52** | **40** | **9**      | **0**     | **33** |

All round-2 blockers are fixed: none of the five unsaved-text loss paths
loses text any more (drafts survive a reload, dirty tabs ask Save / Don't save
/ Cancel, the tab limit keeps dirty tabs, outside writes raise a conflict
instead of replacing edits, Save never recreates a deleted file). Round-1
blockers are fixed too: disconnect is shown within 11 ms and announced, an
offline Save fails with a message and keeps the text; the tree reads level,
position and git status to screen readers.

Measurements, round 2 to round 3: one save with 402 folders open went from 403
`fs.list_dir` requests and a 1,057 ms freeze to 5 requests and no long
frames; a 30,000-line diff from 120,095 DOM elements and a 1,481 ms frame to
506 elements and 113 ms; switching projects from "reload only" to 203 ms;
reaching a deep monorepo file from 98 keystrokes to 7 (Alt+P).

## Open issues, ranked

| #   | Severity | Issue                                                                                                                                                                                                                                                                                                                               | Area                                                              | Findings                                                   |
| --- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ---------------------------------------------------------- |
| 1   | blocker  | Expanding a folder while disconnected freezes the page for good: `model.ts` `pump()`/`due()` keeps re-picking the failed folder and spins on promises that reject at once; never recovers after reconnect. Probably a request storm on other listing errors while connected                                                         | `ui/src/tree`                                                     | A11Y-21, ORCH-20                                           |
| 2   | major    | Nothing re-reads after a reconnect: files created or deleted during the outage stay wrong in the tree, dirty buffers learn of outside changes only on Save, stale banners stay. The core sends `info.resync` only to lagging clients, not to a new socket                                                                           | `ui/src/lib/transport.ts`, tree, editor; maybe `crates/transport` | ORCH-21                                                    |
| 3   | major    | Daemon restart or core up after page: old token retries forever ("Authentication failed", "reconnecting…"), start screen stays empty with a red error until a manual reload; nothing says to open the new URL. A token that survives restarts touches headless access checks: owner decision                                        | `ui/src/shell`, `crates/daemon`                                   | NEW-23, ORCH-22                                            |
| 4   | major    | Focus drops to `<body>` after closing the last tab, Ctrl+E back to view, Go to file while editing, Close project, Escape+Tab out of the editor; Go to file returns focus to the tree, not the file                                                                                                                                  | `ui/src/editor`, `ui/src/palette`, `ui/src/shell`                 | KEY-22, KEY-25, A11Y-14, A11Y-25                           |
| 5   | major    | Close-unsaved question: Tab walks out into the editor, no `aria-modal`, Esc works only on its buttons, it stays up after Ctrl+S and follows to other tabs, and names only the file name (repeated across packages), so "Don't save" can hit the wrong file                                                                          | `ui/src/editor` (Tabs)                                            | MONO-24, KEY-24, A11Y-22                                   |
| 6   | major    | Go to file: Enter typed before the list loads is lost (every first jump after reload); ranking runs on the main thread, 50-125 ms per key at 15k files, 302 ms at 55k; stops at 100 without "100 of N"; weak typo tolerance; "1 results"                                                                                            | `ui/src/palette`                                                  | KEY-23, MONO-21, NEW-26, KEY-26                            |
| 7   | major    | After restoring a big saved tree, Reveal / Go to file / tab switch silently fail to show the file: `TreeModel.expand()` returns early for restored-but-unlisted folders                                                                                                                                                             | `ui/src/tree`                                                     | MONO-19, KEY-20                                            |
| 8   | major    | Big diffs and long text expose only visible rows to screen readers, with no position or total; a reviewer can approve a diff they have not fully heard                                                                                                                                                                              | `ui/src/viewers` (`Lines.svelte`)                                 | A11Y-23                                                    |
| 9   | major    | No visible way to start editing (Ctrl+E or palette only); a tree click drops edit mode                                                                                                                                                                                                                                              | `ui/src/editor`, `ui/src/tree`                                    | NEW-15, MONO-18, KEY-19                                    |
| 10  | minor    | Ctrl+P (print) and Ctrl+S in view mode ("Save page as") are not intercepted in the browser, though pages can block both                                                                                                                                                                                                             | `ui/src/keys`                                                     | KEY-18, NEW-25, MONO-22, ORCH-23                           |
| 11  | minor    | Partly fixed leftovers: reload lands on "No project"; regenerated folder comes back collapsed; cursor keeps a stale offset after outside inserts; conflict banner has no Compare; tabs of deleted/renamed files look normal; dirty tabs can scroll out of view with no count; tab labels carry the whole parent path (3.7 tabs fit) | shell, tree, editor                                               | NEW-3, ORCH-3, ORCH-17, ORCH-15, ORCH-19, ORCH-25, MONO-23 |
| 12  | minor    | Accessibility leftovers: fixed 13 px root font and 260 px sidebar; headings and landmarks; markdown checkboxes unnamed; editor textbox unnamed; save and project switch not announced; contrast (gutter 2.65:1, comments 3.48:1, palette empty text 3.34:1)                                                                         | shell, editor, viewers, palette                                   | A11Y-10, 12, 15, 18, 20, 24, NEW-9                         |
| 13  | minor    | Two browser windows share drafts, last write wins                                                                                                                                                                                                                                                                                   | `ui/src/editor`                                                   | ORCH-26                                                    |

## Suggested tasks

1. **Tree: stop the retry spin** (`ui/src/tree/model.ts`): remember failed
   folders and skip them until the next invalidate or resync; test with a
   transport that rejects. Highest priority, small.
2. **Resync on reconnect** (transport + tree + editor, maybe `crates/transport`
   to send `info.resync` on every new socket): re-list loaded folders,
   re-stat open buffers, clear stale errors; reload the start screen when the
   core comes up.
3. **Daemon restart story** (owner decision): either a stable token per data
   dir or a clear "core restarted, open the new address" screen.
4. **Focus after closing and switching** and **the close-unsaved dialog as a
   real modal** (focus trap, `aria-modal`, path in the question, closes itself
   when the file is saved).
5. **Go to file**: queue Enter until results arrive, rank in a worker or
   incrementally, show "100 of N".
6. **Reveal after restore** (`TreeModel.expand()`), **screen-reader totals in
   virtualized viewers**, **visible Edit button**, **intercept Ctrl+P and
   Ctrl+S in the browser**.
