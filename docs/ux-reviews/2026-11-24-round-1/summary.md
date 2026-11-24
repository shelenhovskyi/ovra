# Round 1 summary (2026-11-24)

Build: `dd5ffce` on `main` plus the UX harness, headless, Chromium, 1280x800.
Scope: shell and status bar, start screen, file tree, command palette, global
shortcuts. Viewer, editor and the project store were stubs at that commit, so
the reviews went through a stand-in for `projects.*` and `fs.*` (since
removed) except where marked "real core". While the round ran, `main` gained
the project store (1.B), viewers (1.E), editor (1.F), open-external (1.G) and
per-project state: issue 3 and part of 8 and 13 are probably fixed; round 2
re-checks them.

Reports: [newcomer](newcomer.md), [keyboard-power-user](keyboard-power-user.md),
[monorepo-maintainer](monorepo-maintainer.md),
[agent-orchestrator](agent-orchestrator.md),
[accessibility-user](accessibility-user.md). 51 findings in total; below they
are merged into 14 issues, grouped by the area that owns the fix and ranked.

## What works

The virtualized tree stays at about 42 rows in the DOM for 5,000 and 50,000
entries and scrolls without dropped frames; live updates for create, modify,
`git mv`, commit and checkout show within 20-160 ms; arrow, Home/End and
Left/Right navigation behave like a native tree; the palette's dialog and
combobox roles are correct and it restores focus; layout survives 200% zoom.

## Issues by area

| #   | Severity            | Issue                                                                                                                                                                                                                                                                              | Area                                                                                    | Findings                                                    |
| --- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| 1   | blocker             | Lost core connection is invisible: status keeps "Connected", calls sent after the close wait forever (tree shows "…" for good), no reconnect, nothing announced                                                                                                                    | `ui/src/lib/transport.ts`, `ui/src/shell`                                               | ORCH-1, A11Y-4                                              |
| 2   | blocker             | Screen readers hear nothing when arrowing the tree: focus stays on the container without `aria-activedescendant`; rows outside the viewport do not exist for browse mode                                                                                                           | `ui/src/tree`                                                                           | A11Y-1                                                      |
| 3   | blocker (real core) | Adding a project on today's `main` shows `unavailable: ... (task 1.B)`; a wrong path gets the same text                                                                                                                                                                            | `crates/store` (1.B), `ui/src/tree`                                                     | NEW-1                                                       |
| 4   | major               | Focus and selection get lost: opening or adding a project drops focus to `<body>`; when the selected row disappears (rename, delete, collapse, wheel scroll) ArrowDown jumps to the top; PageUp/Down scroll without moving the selection; no focus ring before the first arrow key | `ui/src/tree`                                                                           | KEY-2, A11Y-5, MONO-8, ORCH-3, MONO-3, KEY-4, A11Y-8, KEY-5 |
| 5   | major               | Tree rows are wrong for assistive tech and scripts: names include glyphs and badges (`▸ src •`, `main.rs M`), `aria-posinset/setsize` count the whole list, git status and "ignored" are colour or opacity only with no readable attribute                                         | `ui/src/tree`                                                                           | A11Y-2, A11Y-3, ORCH-5, NEW-10                              |
| 6   | major               | Every `fs.git_changed` re-lists every directory ever opened (46 requests for one edit, 320 for ten) and rows flash "…"; a cheaper refresh must still cover the watcher's >512-path batches and ignored folders, which the real watcher does not report                             | `ui/src/tree`, `crates/fs-index` (watch)                                                | MONO-1, ORCH-6, MONO-9, ORCH-9                              |
| 7   | major               | No fast way to a file: Ctrl+P is unhandled (prints the page), no tree typeahead, no collapse all or reveal open file; reaching a deep file took 103 keystrokes                                                                                                                     | `ui/src/palette`, `ui/src/keys`, `ui/src/tree`; file search needs a protocol task first | KEY-1, KEY-3, MONO-4, NEW-5                                 |
| 8   | major               | No way back to the project list or to switch projects; a reload forgets the open project and expanded folders; the palette has one command (itself)                                                                                                                                | `ui/src/shell`, `ui/src/tree`; workspace state is a hot spot                            | NEW-3, ORCH-4, KEY-6, MONO-5                                |
| 9   | major               | Errors show raw codes (`not_found: ...`), give no hint (full path needed, `~` not expanded), never clear; Add gives no progress                                                                                                                                                    | `ui/src/tree` (ProjectList), error text in `ui/src/lib`                                 | NEW-2, NEW-8, A11Y-9                                        |
| 10  | major               | Deleted tracked files vanish without a trace and renames look like new untracked files; the real `fs.list_dir` cannot return a `D` file                                                                                                                                            | protocol `fs` (hot spot), `crates/fs-index`                                             | ORCH-2                                                      |
| 11  | major               | Nothing helps find changes in a big tree: one dot per folder, no counts, no next-change or "changed only" filter                                                                                                                                                                   | `ui/src/tree`                                                                           | MONO-2                                                      |
| 12  | major               | Forced-colours mode hides the palette's matches and selected option                                                                                                                                                                                                                | `ui/src/palette`                                                                        | A11Y-6                                                      |
| 13  | minor               | Main area says only "Viewer" with the path, no "coming soon"; an empty repo shows a lone `.git` instead of "Empty project", and `.git` is the first row everywhere                                                                                                                 | `ui/src/viewers`, `ui/src/editor`, `ui/src/tree`                                        | NEW-4, NEW-6, MONO-8                                        |
| 14  | minor               | Sorting is byte order (`module-1, module-10, ..., module-2`, `README.md` before `a file...`), in the real core too                                                                                                                                                                 | `crates/fs-index` (listing)                                                             | KEY-3, MONO-7                                               |

Smaller items, owner in brackets: contrast below 4.5:1 for hints and ignored
rows (shell, tree; A11Y-7, NEW-9); fixed pixel font sizes and a fixed 260 px
sidebar, names cut at the end with no tooltip (shell, tree; A11Y-10, MONO-6);
the selected row jumps when files are added above it (tree; ORCH-7, MONO-10);
the open path goes stale after rename or delete (shell; ORCH-8); the project
list shows no path and ignores arrow keys, two same-named projects look the
same (tree; NEW-7, KEY-8, ORCH-5); Tab dead-ends in the tree (shell; KEY-7);
palette results and "no match" are not announced (palette; A11Y-11); nothing
shows the Ctrl+Shift+P shortcut and the window title lacks the project (shell;
KEY-9, KEY-10); landmarks and headings (shell; A11Y-12); one 217 ms freeze on
a 50,000-entry listing (tree; MONO-10).

## Suggested tasks

1. **Transport: connection state** (`ui/src/lib/transport.ts`, shell status
   bar): reject pending and new calls when the socket closes, expose a
   connection state, reconnect with backoff, show and announce it (1).
2. **Tree: focus model and semantics** (`ui/src/tree`): keep focus on the tree
   with `aria-activedescendant`, keep or move selection sensibly when rows
   vanish, focus the tree after opening a project, per-sibling
   `posinset/setsize`, clean accessible names, `data-git`/`data-ignored` and a
   text equivalent for status (2, 4, 5).
3. **Tree: incremental refresh** (`ui/src/tree`, with `crates/fs-index`):
   refresh only affected directories, coalesce rebuilds per frame, cover large
   batches and ignored folders (6).
4. **Projects and navigation commands** (shell, tree, palette, keys): "Switch
   project", "Close project", "Collapse all", "Reveal open file", Ctrl+P
   placeholder until file search exists, typeahead in the tree, restore the
   last project on reload (7, 8). Needs a small workspace-state task first.
5. **Start screen errors** (`ui/src/tree` ProjectList): friendly messages per
   error code, a path hint, progress on Add, clear stale errors (3, 9).
6. **Protocol: deleted files in listings and natural sort** (`protocol/fs`,
   `crates/fs-index`): owner decision, it changes a cross-area API (10, 14).
7. **Viewer/editor placeholders** (1.E, 1.F threads): until they land, say
   "Preview coming soon" (13).

Next round re-checks every blocker and major above, and adds the viewers,
editor, project store and open-external as they land.
