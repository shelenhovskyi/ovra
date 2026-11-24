# Marta, the monorepo maintainer: Round 2 (2026-11-24)

Build: `f427218` on `main` (485e5f2 plus the fixture commit), headless, real
Rust core in a **debug build** (`target/debug/ovra-daemon`), Chromium
158.0.8127.36 from Playwright (Linux, Mod = Control), 1280x800.
Scope: viewers (big text, the too-large file, binary, patch view of large
diffs), editor with tabs (30+ files, a long file, highlighting, saving),
projects (switching, per-project state restored on reload and its cost), the
unsaved-changes path, and a re-check of round 1's majors.
Fixtures: `monorepo` (200 packages x 50 files, `generated/` with 5,000 files),
`small` (`demo-app`). Scripts added to `monorepo` before the page loaded:
`tools/big-module.ts` (20,000 lines, 0.9 MB), `tools/server-4_8mb.txt`
(4.8 MB), `tools/too-big-6mb.json` (6 MB), `tools/image.wasm` (200 KB binary),
`reviews/big-2000-files.patch` (`git diff` over 2,000 changed files, 565 KB)
and `reviews/one-file-10k-hunks.diff` (`git diff` of the 20,000-line file with
every other line changed: one hunk, about 30,000 lines). Timings are wall
time in the script unless marked "long frame" (Long Animation Frames API);
backend timings come from a debug build and will be lower in release.

## Summary

Marta can open, read, edit and save files in the monorepo, and the editor
itself copes well with a 20,000-line file (typing stays at frame rate). But
she loses work without a word: closing a tab with edits, Ctrl+W, a reload or
closing the window never asks, the 20-tab cap silently evicts a tab with
unsaved edits, and stashed edits are dropped when an agent touches the file.
The cost of being "a monorepo user" also shows up again: restoring a large
expanded tree blocks the page for about 1.4 s, every save re-lists every
expanded folder (round 1's MONO-1, now hit on every Ctrl+S, one keystroke
took 1,162 ms), big diffs render every line into the DOM (120,000 elements,
a 1.5 s freeze), and twenty tabs all called `module-0.ts` cannot be told
apart.

## Measurements

| What                                                                                         | Result                                                                                       |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Open `monorepo` from the start screen, first row                                             | 226 ms                                                                                       |
| Expand `packages` (200) / `generated` (5,000)                                                | 71 ms / 162 ms, 149 elements on the page, no long frames                                     |
| Viewer: `big-module.ts` (0.9 MB, plain `<pre>`)                                              | 684 ms, one 195 ms long frame                                                                |
| Viewer: `server-4_8mb.txt` (4.8 MB)                                                          | 1,463 ms, long frames 120 + 526 ms                                                           |
| Viewer: `too-big-6mb.json` / `image.wasm`                                                    | 46 ms / 119 ms (message only)                                                                |
| Patch view: 2,000-file `git diff`                                                            | 594 ms, **36,100 elements**, 273 ms long frame                                               |
| Patch view: one-file 30,000-line diff                                                        | 2,821 ms, **120,095 elements**, 1,481 ms long frame                                          |
| Editor (Ctrl+E) on `big-module.ts`, 20,000 lines                                             | 851 ms to editor, first highlight at 1,367 ms, 1,018 elements                                |
| Editor: Ctrl+End then highlight of the new viewport                                          | 126 ms                                                                                       |
| Editor: typing, time from insert to next frame                                               | 6-31 ms (20 keys), no long frames                                                            |
| Save 0.9 MB / 4.8 MB file                                                                    | 396 ms / 569 ms (debug core)                                                                 |
| Open a small file from the tree (30 files in a row)                                          | 85-133 ms; every 5th was 402-425 ms (the step that also collapsed and expanded a package)    |
| Restore on open: 402 saved expanded folders (all 200 packages + `src`, `generated`), 20 tabs | 403 `fs.list_dir`; first row 829 ms; rows settle after ~2.0 s; **14 long frames of ~100 ms** |
| Restore on open: 62 saved expanded folders                                                   | first row 246 ms, settled ~590 ms, one 98 ms long frame                                      |
| Ctrl+S on `README.md` with 101 folders expanded                                              | 102 `fs.list_dir`; one keystroke during the refresh took 103 ms                              |
| Ctrl+S on `README.md` with 402 folders expanded                                              | 403 `fs.list_dir`; long frames 1,057 + 10 x ~100 ms; **one keystroke took 1,162 ms**         |
| External edit of one file with 402 folders expanded                                          | 403 `fs.list_dir`, one 1,240 ms long frame                                                   |

## Findings

### MONO-11 blocker: unsaved edits are lost without any warning

- **Area:** ui/src/editor (session, Tabs, commands); ui/src/shell for the window-level guard
- **Steps:** 1. Open `packages/pkg-000/src/module-0.ts`, Ctrl+E, type
  `// MARTA-EDIT-1`. 2. Click the tab's close button (its label reads "Close
  module-0.ts (unsaved changes)"). 3. Edit `module-1.ts`, press Ctrl+W.
  4. Edit `module-2.ts`, reload the page. 5. Edit again and close the window
  (`page.close({ runBeforeUnload: true })`).
- **Expected:** closing a dirty tab asks "Save / Don't save / Cancel"; a
  reload or window close is held by a `beforeunload` prompt (or the text is
  kept as a draft and restored); at least the edit stays visible somewhere.
- **Actual:** no dialog in any of the four cases (`dialogs=0` in r04
  `session.log`), and nothing reached the disk (`disk A="export const v0 =
  0;"`, B and C the same). After a reload all three edits are gone for good;
  `projects.get_state` brings back the tabs but not the text. Within one
  page session the text survives only in an in-memory stash, and that is
  misleading too:
  - Reopening the file from the tree shows the **disk** text in view mode;
    the edit only reappears after Ctrl+E (r04, CASE1).
  - If the file changes on disk meanwhile (formatter, `git checkout`, an
    agent), the stash is dropped silently on reopen: no notice, no conflict,
    `dirty=0` (r09, STASH; `session.svelte.ts` keeps it only when
    `stash.mtime === file.mtime_ms`).
  - After closing the last dirty tab, the main area keeps a ghost editor
    bar: no path, a `●`, "Rust" and an enabled **Save** button that does
    nothing (r09 GHOST, 11-after-close-dirty.png).
- **Why it matters for Marta:** in an agent-first IDE files under her change
  all the time; she will close tabs in bulk and reload after updates. Silent
  loss of typed work is the one thing that makes her leave an editor.
- **Suggested fix:** confirm on closing a dirty tab (Ctrl+W and the button);
  register `beforeunload` while any file is dirty; when a stash is
  outdated by a disk change, open it as a conflict (the editor already has
  "Overwrite / Reload from disk") instead of dropping it; reset the editor
  bar when no file is open. Longer term, persist drafts in project state so
  a crash or reload keeps them. Unit tests in `session.test.ts` and an e2e
  for "close dirty tab asks".

### MONO-12 major: the 20-tab cap silently closes tabs, including ones with unsaved edits

- **Area:** ui/src/editor (`open-files.svelte.ts`, `MAX_OPEN_FILES`)
- **Steps:** 1. Edit `packages/pkg-000/src/module-0.ts` (dirty). 2. Open 29
  more files from six packages by clicking them in the tree.
- **Expected:** either no hard cap, or the cap skips dirty (and pinned)
  tabs, and she can see that tabs were closed.
- **Actual:** 20 tabs remain; the first ones, including the dirty
  `module-0.ts`, are gone (`hasFirst=false dirtyMarkers=0`, r05). Nothing
  shows that eviction happened or that an edit is now hidden; the edit is
  only found by reopening that exact file and pressing Ctrl+E.
- **Why it matters for Marta:** opening 30 files while tracing a change
  across packages is normal for her; a tab she worked in disappearing is
  both confusing and, with MONO-11, a data-loss path.
- **Suggested fix:** never evict a dirty tab; evict the least recently used
  clean tab rather than the leftmost; consider a higher cap or none, and a
  "Close other tabs / Close saved tabs" command for clean-up.

### MONO-13 major: in a monorepo, tabs and the viewer do not say which file is open

- **Area:** ui/src/editor (Tabs), ui/src/viewers, ui/src/tree (reveal)
- **Steps:** 1. Open `src/module-0.ts` ... `module-12.ts` in six packages
  (30 files). 2. Look at the tab strip. 3. Switch tabs with Ctrl+Tab.
- **Expected:** duplicate names are disambiguated (`module-0.ts
  pkg-003/src`), the active tab is scrolled into view, the main area shows
  the path in view mode too, and the tree can reveal the open file.
- **Actual:** the strip shows `module-0.ts, module-1.ts, module-10.ts,
  module-11.ts, module-12.ts` four times over (20-30-tabs.png); only a hover
  `title` tells them apart. The strip is 2,386 px wide in a 1,020 px area
  and the active tab sits at x = 2,523 px, off screen; after ten Ctrl+Tab
  presses it is still off screen (x = 1,336). View mode shows only the file
  body (04-opened.png), no path, so a `{}` or `export const v0 = 0;` gives no
  clue which package it belongs to. Switching tabs does not reveal or select
  the file in the tree (its package stays collapsed).
- **Why it matters for Marta:** in her repo hundreds of files share a name
  (`index.ts`, `package.json`, `module-N.ts`); she navigates by tabs and
  needs to know where she is.
- **Suggested fix:** add the shortest distinguishing parent folder to
  duplicate tab names, `scrollIntoView({ inline: "nearest" })` on the active
  tab, a path header (or breadcrumb) in the viewer like the editor's bar,
  and "Reveal open file" (also MONO-4).

### MONO-14 major: restoring a large saved tree blocks the page for over a second

- **Area:** ui/src/tree (`model.start`, `rebuild`), possibly crates/fs-index for a batched listing
- **Steps:** 1. Have 402 folders expanded in `monorepo` (every package and
  its `src`, plus `generated`; seeded through `projects.set_state`).
  2. Reload and open `monorepo`.
- **Expected:** the saved shape comes back without a freeze; the open file
  is revealed and selected.
- **Actual:** 403 `fs.list_dir` requests in parallel; each response runs a
  full `rebuild()` over a tree that grows to 15,800 rows, giving 14 long
  frames of ~100 ms back to back (about 1.4 s of blocked main thread) and
  rows settling about 2 s after the click (r06). With 62 folders it is
  fine (590 ms, one 98 ms frame). The active tab (`pkg-171/.../module-3.ts`)
  is not revealed: the tree stays at scrollTop 0 with no selected row. The
  same restore runs every time she switches projects (MONO-17).
- **Why it matters for Marta:** state that survives reload is what she asked
  for in round 1; it now works, but the more she uses it, the slower every
  start gets, which is how her last IDEs failed.
- **Suggested fix:** coalesce rebuilds into one per animation frame (also
  fixes the save case in MONO-1), restore outermost folders first and the
  rest lazily (folders outside the viewport can wait), consider dropping
  saved paths under collapsed ancestors, and reveal the active file after
  restore. Add a perf-gate case: 400 saved folders, budget on longest frame.

### MONO-15 major: big files and big diffs render in full and freeze the viewer

- **Area:** ui/src/viewers (`Viewer.svelte` text view, `PatchView.svelte`)
- **Steps:** 1. Open `tools/server-4_8mb.txt` in view mode. 2. Open
  `reviews/one-file-10k-hunks.diff`. 3. Open `reviews/big-2000-files.patch`.
- **Expected:** the viewer stays responsive (virtualized lines like the
  tree), and a multi-file patch offers a file list or summary to jump
  around.
- **Actual:** the 4.8 MB text goes into one `<pre>`: 1,463 ms to show, a
  526 ms long frame. The 30,000-line diff becomes a table row per line:
  **120,095 elements**, a 1,481 ms long frame, 2.8 s to show. The 2,000-file
  patch makes 36,100 elements and a 273 ms frame, and shows one long page
  with no count of files, no list of them and no way to collapse a file
  (view-big-2000-files.patch.png). Scrolling afterwards is smooth.
- **Why it matters for Marta:** reviewing a codemod or an agent's change
  across packages is exactly a 2,000-file diff; it is the main thing she
  will read in phase 3, and today it is the slowest screen in the app.
- **Suggested fix:** virtualize the text and patch views by line (or show
  large text files through the CodeMirror view read-only, which already
  virtualizes: the editor showed the same 4.8 MB file with 158 elements);
  give the patch view a header with "N files, +a -b" and a file list that
  scrolls to each section, with per-file collapse.

### MONO-16 minor: the too-large and binary messages are a dead end

- **Area:** ui/src/viewers, ui/src/editor
- **Steps:** open `tools/too-big-6mb.json`, `data/huge.txt` (small) or
  `tools/image.wasm`; press Ctrl+E.
- **Expected:** the size and the limit ("6.0 MB, larger than the 5 MB
  limit"), and an action: open in the default app, or show the first part.
- **Actual:** "This file is too large to show." / "This file is too large
  to edit here." / "Binary file (application/wasm) cannot be shown." with
  no size and no action (view-too-big-6mb.json.png). `open.external` exists
  in the protocol, but nothing in the UI calls it.
- **Why it matters for Marta:** big generated JSON, lockfiles and logs are
  common in her repo; she needs a next step, not a wall.
- **Suggested fix:** show the size (already in `FileContent.size`) and a
  button "Open in default app" via `open.external`; later, a "show first
  1 MB" read.

### MONO-17 major: switching projects needs a page reload, and the reload forgets the project

- **Area:** ui/src/shell (workspace), ui/src/palette, ui/src/tree (ProjectList)
- **Steps:** 1. Open `monorepo`. 2. Look for a way to open `demo-app`:
  palette (`project`, `switch`), header, sidebar.
- **Expected:** "Switch project" / "Close project" in the palette or a
  project menu in the header; after a reload the last project reopens.
- **Actual:** the palette has five commands (tabs, toggle edit, itself);
  `project` and `switch` match nothing (r01). The only way back to the
  project list is a page reload, which also drops the open project: the
  sidebar shows the start screen and the main area "Viewer: open a file
  from the tree." (13-after-reload.png). Switching back to `monorepo` then
  pays the whole restore (MONO-14). Expanded folders and open tabs are
  restored per project, which works.
- **Why it matters for Marta:** she alternates between the monorepo and
  small repos all day; "reload to switch" also throws away unsaved edits
  without asking (MONO-11).
- **Suggested fix:** palette commands "Switch project..." and "Close
  project", a click target on the project name in the header, and reopen
  the last project on start (round 1 summary issue 8).

### MONO-18 minor: clicking a file in the tree leaves edit mode

- **Area:** ui/src/tree (`onopen`) with ui/src/shell (workspace.openFile default)
- **Steps:** 1. Open `module-0.ts`, Ctrl+E. 2. Click `module-1.ts` in the tree.
- **Expected:** the next file opens in the mode she is in (tabs already
  keep the mode).
- **Actual:** the main area switches back to view mode
  (failure.png in r05 before the script was adjusted); every file needs
  Ctrl+E again. View mode shows code without highlighting, so going through
  ten files to edit them costs ten extra shortcuts.
- **Why it matters for Marta:** small but constant friction when touching
  the same line in many packages.
- **Suggested fix:** let the tree open files with `workspace.mode`
  (as `show()` in `open-files.svelte.ts` does), or make view mode for code
  the highlighted read-only editor.

## Round 1 re-check

| ID              | Round 1                                           | Now                                                                                                                                                                                                                                                                        |
| --------------- | ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MONO-1          | major: one git change re-lists every loaded dir   | **Still open, and worse.** With the real core every save is an `fs.changed` + `fs.git_changed`; Ctrl+S on `README.md` with 101 folders expanded sent 102 `fs.list_dir`, with 402 folders 403, a 1,057 ms long frame and one keystroke of 1,162 ms (r07, r07b).             |
| MONO-2          | major: no way to find changed files               | **Still open.** Folders show one `•` (title "modified"), no count, no next-change command, no filter.                                                                                                                                                                      |
| MONO-3          | major: lost selection jumps to the top            | **Still open.** Deleting the selected `module-15.ts` from disk, then ArrowDown, selects `.git` (r08). The tab for the deleted file now says "... no longer exists.", which is good.                                                                                        |
| MONO-4          | major: no type-to-find, collapse all, reveal      | **Still open.** Typing `pkg-042` in the tree does nothing; Ctrl+P opens nothing; the palette gained tab and editor commands but no "Collapse all" or "Reveal open file". In a restored 15,800-row tree, reaching `README.md` at the bottom took a script 870 scroll steps. |
| MONO-5          | minor: tree forgets everything on reload          | **Changed / partly fixed.** Expanded folders and open tabs come back per project; the project itself, selection and scroll do not, and the active file is not revealed (MONO-14, MONO-17).                                                                                 |
| MONO-6          | minor: long names, no tooltip, fixed sidebar      | Still open in the tree (no `title`, sidebar 260 px, drag does nothing); tabs do have a full-path `title`.                                                                                                                                                                  |
| MONO-7          | minor: byte-order sort                            | Still open (`module-0, module-1, module-10, ...`).                                                                                                                                                                                                                         |
| MONO-8          | minor: focus on body after open, `.git` first row | Still open (`activeElement` is `BODY`; first row `.git`).                                                                                                                                                                                                                  |
| MONO-9, MONO-10 | minor / polish                                    | Not re-checked this round.                                                                                                                                                                                                                                                 |

## What worked well

- The editor handles a 20,000-line TypeScript file well: about 1,000
  elements in the DOM, highlighting of a new viewport 126 ms after
  Ctrl+End, typing at frame rate, and a 4.8 MB file opens for editing with
  158 elements.
- Saving is quick (under 0.6 s for 4.8 MB on a debug core), the dirty dot on
  the tab and in the editor bar is clear, and the close button's label says
  "(unsaved changes)".
- Expanding 200 and 5,000 entries with the real core takes 71 ms and 162 ms,
  with the tree still at about 150 elements.
- Per-project tabs and expanded folders come back after a reload, and a
  deleted open file shows "no longer exists" instead of stale text.
- Ctrl+Tab / Ctrl+Shift+Tab cycle tabs instantly (10 presses in 365 ms with
  no long frames), and the binary and too-large cases never crash or hang.
