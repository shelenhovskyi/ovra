# Marta, the monorepo maintainer: Round 3 (2026-11-24), regression

Build: `9512ea6` on `main`, headless, real Rust core in a **debug build**
(`target/debug/ovra-daemon`), Chromium 158.0.8127.36 from Playwright (Linux,
Mod = Control), 1280x800.
Scope: re-check of every MONO finding from rounds 1 and 2, with the same
scripts and fixture changes as round 2 so the numbers compare, plus what
landed since `485e5f2`: tree refresh for rows on screen only, virtualized
diffs and text, Go to file over `fs.list_files`, project switcher,
unsaved-text protection (ask on close, drafts across reloads, conflicts),
browser-safe shortcuts.
Fixtures: `monorepo` (200 packages x 50 files, `generated/` with 5,000
files), `small` (`demo-app`). As in round 2, some runs add `tools/big-module.ts`
(20,000 lines), `tools/server-4_8mb.txt`, `tools/too-big-6mb.json`,
`tools/image.wasm`, `reviews/big-2000-files.patch` and
`reviews/one-file-10k-hunks.diff`; one run adds `huge/` with 40,000 untracked
files (55,000 files in the project). Saved state with 402 expanded folders
and 20 tabs is seeded through `projects.set_state`, as in round 2. Other
reviewers ran in parallel on the same machine, so wall times carry some
noise; request counts and DOM sizes do not.

## Summary

Marta can now work in the monorepo the way she wanted: a save or an agent's
edit re-lists 4-5 folders instead of 403, with no long frames and typing at
frame rate; a large saved tree comes back in under 0.7 s; 30,000-line diffs
open with 506 elements; Go to file finds `pkg042mod7` first; and typed text
survives closing, reloading and outside changes. The worst remaining
problem is new: after a large saved tree is restored, Reveal, Go to file
and tab switches silently stop showing the open file in the tree (MONO-19).
Smaller costs remain around Go to file at scale (up to 300 ms per keystroke
with 55,000 files), a stale close question that names only `module-3.ts`,
deleted-file folders drawn as if deleted, wide tab labels, and edit mode
dropping on tree clicks.

## Regression table

Status counts: **8 fixed, 9 partly fixed, 1 still open, 0 regressed** (18
earlier findings). New problems are MONO-19 to MONO-24 below.

| ID      | Round | Old severity | Status       | Evidence / number                                                                                                                                                                                                                                                                                                                                                                                                    |
| ------- | ----- | ------------ | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MONO-1  | 1     | major        | **fixed**    | Ctrl+S on `README.md` with 101 folders expanded: **4** `fs.list_dir` (was 102); with 402: **5** (was 403); external edit with 402: 5 (was 403). No long frames (was 1,057 ms); slowest keystroke right after the save 25 ms (was 1,162 ms) (r07, r07b). Off-screen folders re-list when scrolled back (r14), and a 700-file codegen burst into an expanded `generated/` made 4 requests, no long frames, 5,904 rows. |
| MONO-2  | 1     | major        | partly fixed | "Changed only" toolbar button and palette command: 834 ms, shows all four changes in 13 rows across `pkg-007`, `pkg-042`, `pkg-120`, `pkg-150` (10-changed-only.png); folder titles say "contains modified files". Still no next/previous-change command and no count on folder badges.                                                                                                                              |
| MONO-3  | 1     | major        | **fixed**    | Deleting the selected `module-15.ts` from disk, then ArrowDown, selects `module-16.ts`; scrollTop stays 1,020. PageDown now moves the selection a page (`module-47.ts`) (r11).                                                                                                                                                                                                                                       |
| MONO-4  | 1     | major        | partly fixed | Typing `pkg-042` in the tree selects `packages/pkg-042`; Collapse all, Reveal and Go to file (Alt+P) exist in the toolbar and palette. But Reveal does nothing after a large restore (MONO-19), and Ctrl+P itself is not handled in the browser (MONO-22).                                                                                                                                                           |
| MONO-5  | 1     | minor        | partly fixed | Tabs, expanded folders and unsaved drafts come back per project. A reload still lands on the start screen ("No project", 33-after-reload.png in r13), and the active file is not revealed (MONO-19).                                                                                                                                                                                                                 |
| MONO-6  | 1     | minor        | partly fixed | Rows now have a `title` with the path and status (`packages/pkg-001/a-really-long-....service.client.ts (untracked)`). The name is still cut at the end (572 px of text in 187 px), and the sidebar is still a fixed 260 px with no splitter (drag: 260 -> 260, 0 `role=separator`).                                                                                                                                 |
| MONO-7  | 1     | minor        | **fixed**    | `module-0, module-1, module-2, ... module-7` (natural order).                                                                                                                                                                                                                                                                                                                                                        |
| MONO-8  | 1     | minor        | **fixed**    | Focus is on the tree (`DIV role=tree`) right after opening; `.git` is hidden; first rows are `generated, packages, .gitignore, README.md`.                                                                                                                                                                                                                                                                           |
| MONO-9  | 1     | minor        | partly fixed | A new file in an expanded `packages/pkg-001/dist` (ignored) does not appear within 1.5 s; it appears when the window regains focus. Not live while she watches a build in another window side by side.                                                                                                                                                                                                               |
| MONO-10 | 1     | polish       | partly fixed | Five files created above the view in `generated/` no longer shift it (first visible row `file-02497.json` before and after). The 50,000-entry expand freeze was not re-measured this round.                                                                                                                                                                                                                          |
| MONO-11 | 2     | blocker      | **fixed**    | Close button and Alt+W on a dirty tab show "Save changes to module-0.ts before closing? Save / Don't save / Cancel", Escape cancels; after a reload the draft is back (view says "Showing unsaved changes from the editor.", editor dirty); an outside change to a dirty file shows "changed on disk while you have unsaved changes" with Overwrite / Discard (r12). One stale-question issue: MONO-24.              |
| MONO-12 | 2     | major        | partly fixed | After editing `module-0.ts` and opening 29 more files, the dirty tab survives (`hasFirst=true`, 1 dirty marker). Clean tabs are still evicted silently from the left at 20, not least recently used, with no notice.                                                                                                                                                                                                 |
| MONO-13 | 2     | major        | partly fixed | Tabs are disambiguated (`module-0.ts - packages/pkg-000/src`), the viewer shows the path, the active tab is scrolled into view and switching tabs selects the file in the tree. Remaining: very wide labels and a clipped active tab (MONO-23).                                                                                                                                                                      |
| MONO-14 | 2     | major        | **fixed**    | Restore of 402 saved folders + 20 tabs: **3** `fs.list_dir` (was 403), first row 383 ms (829), rows settled ~690 ms (~2.0 s), **0 long frames** (14 of ~100 ms) (r06). The missing reveal is tracked as MONO-19.                                                                                                                                                                                                     |
| MONO-15 | 2     | major        | **fixed**    | 30,000-line diff: 479 ms, **506 elements** (was 2,821 ms, 120,095), longest frame 113 ms (1,481). 2,000-file patch: 254 ms, 2,429 elements (36,100), header "2001 files changed, +4001 -2001", Go to file list, Collapse all / Expand all, per-file collapse. 4.8 MB text: 684 ms, 161 elements, longest frame 145 ms (526).                                                                                         |
| MONO-16 | 2     | minor        | **fixed**    | "This file is 6.0 MB, too large to show here. Open in default app"; "This is a binary file (application/wasm, 195 KB) and cannot be shown here. Open in default app"; every viewer header has "Open in default app".                                                                                                                                                                                                 |
| MONO-17 | 2     | major        | partly fixed | Palette "Switch project" (40 ms to `demo-app`, 203 ms back to `monorepo` with 20 tabs restored, 3 `fs.list_dir`, no long frames) and "Close project" work. A reload still forgets the open project.                                                                                                                                                                                                                  |
| MONO-18 | 2     | minor        | still open   | Editing `module-0.ts`, then clicking `module-1.ts` in the tree: mode `view` (r12).                                                                                                                                                                                                                                                                                                                                   |

## Measurements (round 2 vs round 3)

Same scripts, fixture changes and selectors as round 2 (only the project
button locator and the browser-safe tab keys were adapted).

| What                                                              | Round 2                                                   | Round 3                                                                                                        |
| ----------------------------------------------------------------- | --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| Open `monorepo` from the start screen, first row                  | 226 ms                                                    | 237-341 ms (3 runs)                                                                                            |
| Expand `packages` (200) / `generated` (5,000)                     | 71 / 162 ms, 149 elements                                 | 119-150 / 255-388 ms, 151 elements, no long frames; the core's `generated` listing took 125-261 ms (was 94 ms) |
| Viewer: `big-module.ts` (0.9 MB)                                  | 684 ms, long frame 195 ms                                 | 370 ms, 158 elements, long frames 54 + 75 ms                                                                   |
| Viewer: `server-4_8mb.txt` (4.8 MB)                               | 1,463 ms, long frames 120 + 526 ms                        | 684 ms, 161 elements, long frames 90 + 145 ms                                                                  |
| Viewer: `too-big-6mb.json` / `image.wasm`                         | 46 / 119 ms, message only                                 | 112 / 154 ms, size + "Open in default app"                                                                     |
| Patch view: 2,000-file `git diff`                                 | 594 ms, **36,100 elements**, 273 ms long frame            | 254 ms, **2,429 elements** (2,001 are the file picker's options), 86 ms long frame                             |
| Patch view: one-file 30,000-line diff                             | 2,821 ms, **120,095 elements**, 1,481 ms long frame       | 479 ms, **506 elements**, long frames 59 + 113 ms                                                              |
| Scrolling 60 x 2,000 px through each of the above                 | smooth                                                    | no long frames; DOM stays at 189-2,575 elements                                                                |
| Editor (Ctrl+E) on `big-module.ts`                                | 851 ms, highlight 1,367 ms, 1,018 elements                | 359 ms, highlight 1,075 ms, 1,026 elements                                                                     |
| Editor: Ctrl+End, highlight of new viewport                       | 126 ms                                                    | 70 ms                                                                                                          |
| Editor: typing, insert to next frame                              | 6-31 ms                                                   | 7-32 ms                                                                                                        |
| Save 0.9 MB / 4.8 MB                                              | 396 / 569 ms                                              | 232 / 650 ms                                                                                                   |
| Open a small file from the tree (30 files)                        | 85-133 ms; every 5th 402-425 ms (collapse/expand step)    | 82-120 ms; every 5th 399-414 ms (same step)                                                                    |
| Restore: 402 saved folders, 20 tabs                               | 403 `fs.list_dir`, first row 829 ms, ~2.0 s, 14 x ~100 ms | **3** `fs.list_dir`, first row 383 ms, ~690 ms, **0 long frames**; open file not revealed                      |
| Ctrl+S on `README.md`, 101 folders expanded                       | 102 `fs.list_dir`, one key 103 ms                         | **4** `fs.list_dir`, slowest key 21 ms, 0 long frames                                                          |
| Ctrl+S on `README.md`, 402 folders expanded                       | 403 `fs.list_dir`, 1,057 ms frame, one key 1,162 ms       | **5** `fs.list_dir`, slowest key 25 ms, 0 long frames                                                          |
| External edit of one file, 402 folders expanded                   | 403 `fs.list_dir`, 1,240 ms frame                         | **5** `fs.list_dir`, 0 long frames                                                                             |
| Change in an expanded folder off screen, then scroll back         | (re-listed at once)                                       | re-listed at the scroll (request 0 ms after, answer 8-10 ms); `D` badge right                                  |
| 700 files written into expanded `generated/` (watcher batch >512) | (round 1: 46 requests)                                    | 4 `fs.list_dir`, 0 long frames, 5,904 rows                                                                     |
| Switch project                                                    | reload only                                               | palette: 40 ms to `demo-app`, 203 ms back (20 tabs, 402 saved folders, 3 `fs.list_dir`)                        |
| Ctrl+Tab x 10 (now Alt+] in the browser)                          | 365 ms, no long frames                                    | 385 ms, no long frames                                                                                         |
| Go to file, cold, 15,200 files                                    | n/a                                                       | dialog 10-18 ms, list 112-119 ms (one `fs.list_files`); warm 14 ms                                             |
| Go to file, per keystroke, 15,200 files                           | n/a                                                       | 50-125 ms for the first 3-5 characters (long frames 55-97 ms), 12-40 ms after                                  |
| Go to file, 55,200 files (`huge/` added)                          | n/a                                                       | cold 235 ms (two pages); per keystroke 42-302 ms (`chunk-39999`: 302, 194, 164, 223, 219, 221, 208 ms ...)     |
| "Changed only" in the monorepo with 4 changes                     | n/a                                                       | 834 ms, 13 rows, 0 long frames                                                                                 |

Go to file ranking (15,200 files): `pkg042mod7` and `pkg-042/src/module-7`
put `packages/pkg-042/src/module-7.ts` first, then `module-17/27/37/47`;
`42/7` gives `pkg-042/.../module-7.ts`, then `pkg-142/.../module-7.ts`;
`readme`, `file-04999` and `packjson` all hit first; `pkg-7/` ranks
`pkg-007/package.json` first. `module-7` and `mod7.ts` show exactly 100
rows (the cap) with no hint that 100 more exist. `index` finds nothing,
because `dist/` is ignored, and the empty state does not say ignored files
are left out. A new file written on disk is found in the next search. With
an empty query the list starts at `.gitignore`, `README.md`, then
alphabetical: no recent or open files first.

The slower `generated` listing (core 125-261 ms against 94 ms in round 2) is
worth a look in the perf gate with a release build: the core now sorts
naturally and lists deleted files, and the machine was shared this round,
so it is not filed as a finding.

## New findings

### MONO-19 major: after a large restore, Reveal, Go to file and tab switches do not show the file in the tree

- **Area:** ui/src/tree (`model.ts` `reveal` / `expand`, lazy restore in `setView`)
- **Steps:** 1. Have `monorepo` saved with `generated`, `packages` and every
  `pkg-NNN` and `pkg-NNN/src` expanded, and the active tab
  `packages/pkg-171/src/module-3.ts` (402 folders, 20 tabs). 2. Open the
  project and wait 2.5 s. 3. Press Reveal in the tree toolbar. 4. Alt+P,
  `pkg100mod8`, Enter. 5. Alt+[ to the previous tab.
- **Expected:** the tree scrolls to the open file and selects it each time,
  loading the folders on the way.
- **Actual:** nothing happens in any of the three cases: no selected row,
  scrollTop stays 0, the tree keeps showing `generated/file-00000.json ...`
  (r13 31-revealed.png; r16 `GOTO after restore ... selected=none
  scrollTop=0`, `TABSWITCH ... selected=none`). There is no message. The
  workaround is Collapse all, then Reveal, which works (`REVEAL after
  collapse-all selected=packages/pkg-171/src/module-3.ts`), but it throws
  away the restored shape. Cause, from the code: restored folders are put
  in the expanded set but listed only when their row comes on screen, and
  `TreeModel.expand()` returns early for a path already in that set, so
  `reveal()` never loads `pkg-171/src` and the row does not exist.
- **Why it matters for Marta:** the bigger her saved tree, the more likely
  the open file is in a folder that is restored but not yet listed; then
  every way to find "where am I" fails silently, right after the start.
- **Suggested fix:** in `expand()` (or in `reveal()`), load a folder that
  is expanded but not yet listed instead of returning; then select and
  centre the row. Unit test in `model.test.ts`: restore with the target's
  folders off screen, `reveal()` yields a row for the file.

### MONO-20 minor: a folder that contains a deleted file is struck through as if it were deleted

- **Area:** ui/src/tree (row styling for `git = deleted` on folders)
- **Steps:** 1. Delete `packages/pkg-120/src/module-1.ts` from a terminal.
  2. Look at `packages/pkg-120` and `packages/pkg-120/src`, in the full tree
  and with "Changed only".
- **Expected:** only the deleted file is struck through; its folders show a
  change marker (the dot), since 49 files are still there.
- **Actual:** both folders render red with `text-decoration: line-through`
  (10-changed-only.png, 50-strike.png), the same as a deleted file. The
  `title` is right ("contains deleted files"), but the visual says "this
  package is gone".
- **Why it matters for Marta:** in "Changed only" she scans for what was
  removed; a struck-through package makes her think an agent deleted a whole
  package and sends her checking.
- **Suggested fix:** keep the strike-through for file rows (and folders
  that are deleted themselves, if the core can say so); give folders only
  the coloured dot. A component test on a folder with `git: "deleted"`.

### MONO-21 minor: Go to file blocks typing for up to 300 ms per key in a large repo

- **Area:** ui/src/palette (`fuzzy.ts` `rank`, `Palette.svelte`)
- **Steps:** 1. Open `monorepo` (15,200 files), Alt+P, type `module-7`.
  2. Add 40,000 files under `huge/` (55,200 files), Alt+P, type
  `chunk-39999`.
- **Expected:** each keystroke under one frame or close to it, as in her
  old IDEs' quick open; a hint when the list is cut.
- **Actual:** with 15,200 files every one of the first 3-5 keys costs
  50-125 ms (long animation frames of 55-97 ms); with 55,200 files
  42-302 ms per key, seven keys in a row over 160 ms. The whole list is
  ranked on the main thread for every key, from scratch even when the new
  query only extends the old one. Results stop at 100 rows with no
  "100 of N" and no hint; the empty query shows `.gitignore` first rather
  than recent files.
- **Why it matters for Marta:** Go to file is now how she gets around; a
  stutter that grows with the repo is the pattern that drove her off two
  IDEs, and real monorepos reach 100,000+ files.
- **Suggested fix:** when the query extends the previous one, rank only the
  previous matches; skip the full alignment for candidates that cannot beat
  the current 100th score; or rank in a worker. Show "100 of 1,234" when
  cut, and recent/open files for an empty query. Add a perf-gate case with
  50,000 paths and a budget per keystroke.

### MONO-22 minor: Ctrl+P is not handled in the browser, so it opens the print dialog

- **Area:** ui/src/keys (`shortcut.ts` `BROWSER_SAFE`)
- **Steps:** 1. In headless mode, open `monorepo`. 2. Press Ctrl+P.
- **Expected:** Go to file opens (the shortcut every editor she uses has),
  with Alt+P as an extra.
- **Actual:** no palette (`dialog=0`) and the keydown is not prevented
  (`defaultPrevented=false`), so a real Chrome opens Print. The palette
  lists Go to file as Alt+P only. Unlike Ctrl+W and Ctrl+Tab, which a page
  cannot take over, Chrome lets a page prevent Ctrl+P (and Ctrl+S, which
  Ovra already uses).
- **Why it matters for Marta:** Ctrl+P is muscle memory; the print dialog
  on a 15,000-file project is the opposite of what she asked for.
- **Suggested fix:** keep `Mod+P` live in the browser (drop it from
  `BROWSER_SAFE`, or bind both), show Ctrl+P in the palette; keep the
  stand-ins only for shortcuts the browser really reserves.

### MONO-23 minor: tab labels carry the whole parent path, so only three or four tabs fit

- **Area:** ui/src/editor (Tabs)
- **Steps:** 1. Open 30 files from six packages (`module-0/1/10/11/12.ts` in
  `pkg-000` ... `pkg-005`). 2. Cycle with Alt+].
- **Expected:** the shortest distinguishing part (`module-0.ts pkg-003`),
  and the active tab fully visible, close button included.
- **Actual:** each label is `module-0.ts - packages/pkg-004/src`, about
  275 px; the strip is 5,477 px wide in 1,020 px, so 3.7 tabs are visible.
  The active last tab ends at x = 1,306 in a 1,280 px window: its close
  button is cut off (22-ctrltab.png).
- **Why it matters for Marta:** she keeps many tabs; scanning them is now
  possible but slow, and the useful part (`pkg-004`) sits in the middle of
  each label.
- **Suggested fix:** show only the shortest parent suffix that tells
  duplicates apart (VS Code style), muted, with the full path in the
  `title`; make sure `scrollIntoView` targets the whole tab including its
  close button (or pad the strip's end).

### MONO-24 major: the close question stays up after the file is saved and follows her to other tabs, naming only the file name

- **Area:** ui/src/editor (`open-files.svelte.ts` `tabs.closing`, Tabs)
- **Steps:** 1. Restore 20 tabs all named `module-3.ts` (different
  packages). 2. Edit the active one, press Alt+W: "Save changes to
  module-3.ts before closing?". 3. Instead of answering, click into the
  editor and press Ctrl+S. 4. Click another `module-3.ts` tab.
- **Expected:** saving answers the question (or closes it); switching tabs
  cancels it; the question names the folder when names repeat.
- **Actual:** after Ctrl+S the file is clean (`dirty=0`) but the question is
  still shown; it stays above the other tab's editor (`packages/pkg-009/...`)
  with the same text (60-stale-question.png, r16; the same in r09 after
  opening `data/huge.txt`). With two dirty files of the same name she cannot
  tell which one "Don't save" will discard; it discards the first one, not
  the one on screen.
- **Why it matters for Marta:** in her repo most names repeat; a question
  that outlives its context and does not say which file is how typed work
  gets thrown away by mistake.
- **Suggested fix:** clear `tabs.closing` when that file becomes clean or
  another tab is shown; put the disambiguated label (or path) in the
  question; consider anchoring it to the tab it is about. Unit test in
  `open-files` for both.

## What worked well

- The refresh rework is exactly what she asked for in round 1: cost now
  follows what is on screen, not what she ever opened (5 requests instead of
  403 per save), folders off screen catch up the moment they scroll in, and
  a 700-file codegen burst causes no visible work.
- Restoring a 402-folder tree and switching projects with the palette are
  quick (383 ms to first row, 203 ms to switch back) and never block a
  frame.
- Big diffs are now pleasant to read: a summary line, a file picker,
  collapse per file and for all, line numbers on both sides, and the DOM
  stays at a few hundred elements for 30,000 lines.
- Go to file ranks well on path fragments (`pkg042mod7`, `42/7`,
  `pkg-7/`), loads 15,000 paths in about 115 ms, sees new files, and opening
  a result reveals and selects it in the tree (outside the MONO-19 case).
- Unsaved text is safe: a real Save / Don't save / Cancel question, drafts
  that survive a reload and show in the viewer with a note, and a clear
  conflict with "Overwrite with my version / Discard my changes and reload"
  when an agent changes the file underneath.
- Tree keyboard work feels right: focus on open, type-ahead, PageUp/Down
  move the selection, a lost row keeps her place, natural order, `.git`
  out of the way.
