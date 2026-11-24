# Marta, the monorepo maintainer: Round 1 (2026-11-24)

Build: dd5ffce on `main` plus the UX harness, headless, Chromium bundled with
Playwright 1.63.0 (Linux, Mod = Control), 1280x800.
Scope: shell layout, start screen / project list, virtualized file tree with
git status and live updates, command palette, global shortcuts.
Fixtures: `monorepo` (200 packages x 50 files, `generated/` with 5,000 files),
`small` (`demo-app`); one run added `huge/` with 50,000 files and one run a
110-character file name. Listings came from the harness stand-in
(`tests/e2e/ux/fake-core.ts`), so backend latency is not judged here; all
timings below are UI-side (DOM size, frames, long animation frames, request
count) unless marked "wall".

## Summary

Marta could open the monorepo, expand the 5,000-file folder and a 50,000-file
folder, and scroll them smoothly: the tree keeps about 40-50 rows in the DOM
(150-190 elements on the whole page) and never dropped a frame while
scrolling. What hurts is getting around: there is no type-to-find, no
"collapse all", no "reveal open file", no way to find the one changed file
among 200 packages except scrolling for a dot, and the tree forgets
everything on reload. Live updates arrive quickly, but every git status
change re-lists every directory she ever opened, and losing the selected row
(a file deleted by another tool) throws her back to the top of the tree.

## Measurements (for the perf gate, task 1.H)

| What                                                                                       | Result                                                          |
| ------------------------------------------------------------------------------------------ | --------------------------------------------------------------- |
| DOM after expanding `generated/` (5,005 rows)                                              | 42 tree rows, 148 elements on the page                          |
| DOM after expanding `huge/` (50,000 rows)                                                  | 42 tree rows, 188 elements; spacer 1,100,130 px tall            |
| UI time from `fs.dir` frame (5,000 entries) to rows in DOM                                 | about 35 ms (frame at 29.530, rows at 29.566 in `session.log`)  |
| Main thread on the 50,000-entry frame                                                      | one long animation frame of 217 ms (parse + `rebuild` + render) |
| Programmatic scroll, 120 frames x 600 px over 5,200 rows                                   | worst frames 17 ms, no long animation frames                    |
| Programmatic scroll, 120 frames x 9,000 px over 50,000 rows                                | worst frame 19 ms                                               |
| 200 ArrowDown keydowns in a 5,200-row tree                                                 | 15 ms total                                                     |
| 100 ArrowUp keydowns at the end of a 50,000-row tree                                       | 124 ms total (1.2 ms each: `rows.findIndex` per key)            |
| Collapse/expand cached `generated/` (5,700 rows)                                           | about 17 ms each, one frame                                     |
| `fs.list_dir` requests after touching one file (`README.md`) with 22 dirs loaded           | 46 (one `fs.changed` + one `fs.git_changed`; see MONO-1)        |
| Live update, wall time from write on disk to row in DOM (stand-in watcher, 50 ms debounce) | create 376-400 ms, modify (badge) 350-390 ms, delete 380-391 ms |

## Findings

### MONO-1 major: one git status change re-lists every loaded directory

- **Area:** ui/src/tree (with crates/fs-index for the event contract)
- **Steps:** 1. Open `monorepo`, expand `packages`, ten `pkg-NNN/src`
  folders and `generated/`. 2. From a terminal, change one tracked file
  (`README.md`). 3. Count `fs.list_dir` frames in `session.log`.
- **Expected:** the tree reloads the folder that changed and updates git
  badges; cost grows with what changed, not with how much she has expanded.
- **Actual:** `fs.git_changed` makes `TreeModel.refreshAll()` re-list every
  loaded directory: 46 `fs.list_dir` requests for one edit, including the
  5,000-entry `generated/` (out03 `session.log`, lines after
  `ACTION burst-start`). Each response also runs a full `rebuild()` of all
  rows (5,700 here). A codegen run writing 500 files produced the same 46
  requests plus one for `generated/`. The UI itself stayed responsive at this
  size (no long animation frames), but with the real core every request is a
  directory read plus `.gitignore` matching, and the first edit to a clean
  file, every `git add`, commit, checkout or stash triggers it. In a session
  where she has opened a few hundred folders, or an agent is committing,
  this is hundreds of listings per event. With 50,000 rows loaded, a single
  rebuild already blocks for about 200 ms (see table), so 46 rebuilds in a
  row is a visible freeze.
- **Why it matters for Marta:** this is exactly how her last two IDEs froze:
  fine on open, then slow once she has been working for an hour.
- **Suggested fix:** have the core send changed git statuses (path ->
  status, or the list of paths whose status changed) with `fs.git_changed`
  and patch them into the loaded listings instead of re-listing; at minimum,
  refresh only the expanded directories whose subtree contains a changed
  path, and coalesce the `rebuild()` calls into one per animation frame.
  Note for the fix: the real watcher reports batches over 512 paths as their
  top-level entries (`crates/fs-index/src/watch.rs`, `MAX_PATHS`), and
  `refreshPaths` then reloads only that top-level folder and the root, not
  the loaded folders under it. Today `refreshAll` hides this; a cheaper
  refresh must treat a top-level path as "reload every loaded folder below
  it". Add a perf-gate case: 200 expanded folders, one `fs.git_changed`,
  budget on requests and main-thread time.

### MONO-2 major: no way to find changed files in a big tree

- **Area:** ui/src/tree (commands registered through ui/src/keys)
- **Steps:** 1. Open `monorepo`. 2. Try to find the one modified file
  (`packages/pkg-042/src/module-7.ts`).
- **Expected:** a way to jump to the next/previous changed file, or a
  "changed files only" filter, or at least a count per folder.
- **Actual:** the root shows `packages` with an orange dot and nothing else
  (02-root.png). She has to expand `packages`, scroll 42 rows to find the
  one `pkg-042` with a dot, expand it, then `src`, then scroll again; in
  20-pkg042.png `module-7.ts` is below the fold because names sort as
  `module-0, module-1, module-10, ...`. Folder dots carry no count and only
  the colour of the "worst" status, so `packages` turns red after one
  deletion and she cannot tell whether one or fifty files changed.
- **Why it matters for Marta:** "see what I touched" is one of her three
  goals for a session, and git status is how she finds her way back.
- **Suggested fix:** commands `tree.nextChange` / `tree.previousChange`
  (expand ancestors and select), and a "changed only" toggle in the tree
  header; show a small count on folder badges. A full Changes panel can come
  later; these two cover today's flow.

### MONO-3 major: losing the selected row sends keyboard navigation to the top

- **Area:** ui/src/tree
- **Steps:** 1. Expand `packages/pkg-042/src` and select `module-1.ts`
  (scrollTop 1150). 2. Delete it from a terminal (or let an agent rename it,
  or `git checkout` another branch). 3. Press ArrowDown.
- **Expected:** selection moves to the next sibling (or the parent), the
  scroll position stays.
- **Actual:** selection jumps to `.git`, the first row, and the tree scrolls
  to the top (scrollTop 0; 60-after-delete-down.png). The same happens after
  PageDown: PageDown scrolls natively without moving the selection, and the
  next ArrowDown snaps back to the old row (scrollTop 879 -> 503). It also
  happens whenever the selected row's ancestor is collapsed: `onkeydown`
  computes `rows.findIndex(...)` and treats "not found" as index -1.
- **Why it matters for Marta:** in a 200-package tree, "back to the top" is
  a lost minute every time, and in an agent-first IDE files under her
  selection change all the time.
- **Suggested fix:** when the selected path disappears from `rows`, select
  the row now at its old index (or its nearest visible ancestor) inside
  `TreeModel`; handle PageUp/PageDown by moving the selection one viewport
  instead of only scrolling. Unit-test both in `model.test.ts` /
  `FileTree.test.ts`.

### MONO-4 major: no type-to-find, no collapse all, no reveal; palette has one command

- **Area:** ui/src/tree (commands), ui/src/palette
- **Steps:** 1. Focus the tree with `packages` expanded and type `pkg-042`.
  2. Press Ctrl+Shift+P and search `collapse`, `reveal`, `project`, `file`.
  3. Press Ctrl+P.
- **Expected:** typing jumps to the first matching row; the palette offers
  "Collapse all folders", "Reveal open file in tree", "Switch project";
  Ctrl+P opens files by name.
- **Actual:** typing does nothing. The palette lists one command, "Show
  command palette Ctrl+Shift+P"; every search shows "No matching commands"
  (53-palette.png). Ctrl+P does nothing. After she collapses `pkg-042`, the
  file still shown in the main area (`module-10.ts`) cannot be found again
  except by expanding by hand. With 5,000 files in `generated/` expanded,
  the folders below it (`packages`, then `README.md`) are 5,000 rows away and
  the only way back up is scrolling or Home + clicking the folder.
- **Why it matters for Marta:** she navigates deep folders all day;
  without type-ahead and collapse-all, each trip through `packages/` costs
  more than in the IDEs she left.
- **Suggested fix:** register `tree.collapseAll` and `tree.revealOpenFile`
  through `registerCommand` (no shared-type change needed), add prefix
  type-ahead on the tree (`aria` treeview pattern), and plan quick-open by
  file name (it needs a core-side file index; likely its own task).

### MONO-5 minor: the tree forgets everything on reload

- **Area:** ui/src/shell (with ui/src/tree for expanded state)
- **Steps:** 1. Open `monorepo`, expand `packages` and `pkg-007/src`, open a
  file. 2. Reload the page.
- **Expected:** the same project, expanded folders, selection and scroll
  position.
- **Actual:** back to the start screen with "No project" (54-after-reload.png);
  after reopening, every folder is collapsed. With the project store still a
  stub (1.B) the project part is expected; the expanded-folder state is not.
- **Why it matters for Marta:** she explicitly expects state to survive a
  reload; rebuilding a 6-level-deep expansion after every restart is the
  kind of friction she notices.
- **Suggested fix:** persist expanded paths, selection and scrollTop per
  project (for now in `localStorage` keyed by project id), restore them
  after the root listing, and drop paths that no longer exist. Reopen the
  last project on start once 1.B lands.

### MONO-6 minor: long names are cut at the end, with no tooltip

- **Area:** ui/src/tree (and ui/src/shell for the sidebar width)
- **Steps:** 1. Add
  `packages/pkg-007/src/a-really-long-generated-module-name-from-the-protobuf-codegen-pipeline.service.client.ts`.
  2. Expand the folder and hover the row for a second. 3. Try to drag the
  sidebar's right edge.
- **Expected:** the name truncated in the middle so the suffix
  (`.service.client.ts`) stays visible, a tooltip with the full path, and a
  sidebar that can be made wider.
- **Actual:** the row shows `a-really-long-generated-…` (51-longname.png,
  572 px of text in 173 px); neither the row nor the name has a `title`, so
  hovering shows nothing. The sidebar is a fixed 260 px grid column with no
  splitter; dragging the border does nothing. Generated files that share a
  long prefix are indistinguishable.
- **Why it matters for Marta:** generated code in her repo has exactly these
  names; she named readable long names as a must.
- **Suggested fix:** middle truncation (keep the extension and the last
  segment), `title` with the project-relative path on each row, and a
  resizable sidebar whose width is remembered.

### MONO-7 minor: numbered files sort as 1, 10, 11, ..., 2

- **Area:** crates/fs-index (`listing.rs`; the stand-in copies it)
- **Steps:** expand any `packages/pkg-NNN/src`.
- **Expected:** `module-0, module-1, module-2, ..., module-10` (natural
  order, as in most file managers and IDEs).
- **Actual:** byte order: `module-0, module-1, module-10 ... module-19,
  module-2, module-20` (20-pkg042.png). `listing.rs` sorts with
  `a.name.cmp(&b.name)`; capitals also sort before lowercase.
- **Why it matters for Marta:** numbered and versioned files (migrations,
  shards, generated chunks) are common in her repo; byte order makes them
  hard to scan.
- **Suggested fix:** natural, case-insensitive ordering in
  `fs-index` listing (dirs first stays), with an `insta` snapshot.

### MONO-8 minor: opening a project leaves focus on the page, and `.git` is the first row

- **Area:** ui/src/tree
- **Steps:** 1. On the start screen click `monorepo`. 2. Press ArrowDown.
  3. Press Tab, then Home.
- **Expected:** focus goes to the tree, so arrows work at once; `.git` is
  hidden (or at least not where Home lands).
- **Actual:** focus is on `BODY` after opening, ArrowDown does nothing;
  Tab focuses the tree. Home then selects `.git`, a folder she never browses
  (dimmed, but always the first row; 02-root.png).
- **Why it matters for Marta:** small, but it happens on every project open.
- **Suggested fix:** focus the tree when a project opens; hide `.git` by
  default (a setting later), or at least sort it after the visible folders.

### MONO-9 minor: expanded ignored folders go stale with the real core

- **Area:** crates/fs-index (watcher) and ui/src/tree
- **Steps (from code, the stand-in watches everything):** 1. Expand
  `packages/pkg-007/dist` (dimmed, correct). 2. Run a build that adds files
  to `dist/`.
- **Expected:** the expanded folder updates, or shows that it is not live.
- **Actual:** `crates/fs-index/src/watch.rs` watches only non-ignored
  directories, and ignored files do not change git status, so no
  `fs.changed` or `fs.git_changed` arrives; the listing stays as it was until
  some unrelated git change triggers MONO-1's full reload.
- **Why it matters for Marta:** she wants `dist/` and `node_modules/` quiet
  but reachable; checking build output in the tree and seeing an old listing
  misleads her.
- **Suggested fix:** watch ignored directories while they are expanded in a
  client (or re-list them on expand and on window focus), and keep MONO-1's
  fix from relying on unrelated events to refresh them.

### MONO-10 polish: inserted rows shift the view; one 200 ms freeze on a 50k listing

- **Area:** ui/src/tree
- **Steps:** 1. Scroll to the middle of `generated/`. 2. Create
  `generated/a-inserted.json`. Separately: expand a folder with 50,000 files.
- **Expected:** rows under her eyes stay put; expanding never blocks a frame
  for long.
- **Actual:** the first visible row changes from `file-02498.json` to
  `file-02497.json` (content moves by one row for every file added above).
  Expanding `huge/` produced one 217 ms long animation frame. While the
  listing loads, the only feedback is a small `…` after the folder name
  (15 s wall with the stand-in; the real core will be faster, but a large
  listing over a slow disk will show the same thing).
- **Why it matters for Marta:** codegen running in the background makes the
  view crawl; the freeze is the first sign of the problem she left her old
  IDEs over.
- **Suggested fix:** keep the top visible row anchored when rows are
  inserted above it; for 1.H, add a 50,000-entry listing case with a budget
  on the longest frame, and consider building rows incrementally (only the
  changed folder's slice instead of a full `rebuild()` walk).

## What worked well

- Virtualization is solid: 5,000 and 50,000 rows stay at about 42 DOM rows,
  scrolling is at 60 fps in both, and keyboard navigation is instant at
  5,000 rows.
- Expanding a 5,000-file folder renders in about 35 ms after the frame
  arrives, and collapsing/expanding a loaded folder takes one frame.
- Git status propagates to parent folders (`packages` shows a dot for a
  change three levels down) and ignored folders (`dist/`) are dimmed but
  still expandable.
- Live updates work end to end: created, modified and deleted files appear
  with the right badge (`U`, `M`) and the row goes away on delete; a refresh
  keeps her selection and scroll position (apart from the one-row shift in
  MONO-10, and as long as the selected row survives, see MONO-3).
- The start screen shows each project's full path as a tooltip, which tells
  two checkouts of the same repo apart.
