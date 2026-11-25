# Git panel round: re-check of the majors

Build: a98448d, headless, Playwright's bundled Chromium, 1280x800 (one pass
at 640x400).
Scope: E1-E4 and P1-P6 from `summary.md`, and their findings; a quick look
for regressions in commit, stage, the diff tab, editor markers, Alt+F5, the
change popup and blame.
Personas: `keyboard-power-user` (Oleh) and `accessibility-user` (Iryna).
Fixtures: `small` (seeded). Each script also used real git to commit a
40-line `src/long.rs`, `src/mod_a.rs` and `src/mod_b.rs`, then changed them.
It also made a branch `other` that changes `src/main.rs`. One run added two
lines and deleted one in `long.rs`.

## Summary

Eight of the ten majors are fixed, and the fixes hold up when used by
keyboard and screen reader. Markers are visible, Alt+F5 announces each
change, the popup traps focus and closes cleanly, and the diff is a real
selected tab. The rows show diffs, each list is one Tab stop, and focus comes
back to the row. P6 is still open: "Git: show changes" and "Git: show change
at cursor" still share a prefix, and "show change" now runs a tree toggle.
P5 is worse than before for commits with a body: one Ctrl+Enter after
ticking Amend drops the body.

## Status of the original majors

| ID  | Status                  | Evidence                                                                                                                                                                                                                                                                                                           |
| --- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| E1  | fixed                   | Markers measure `modified 4.0x19.5`, `added 4.0x19.5`, `deleted 6.0x10.0` and show as a striped bar, a solid bar and a red triangle (`21-popup.png`).                                                                                                                                                              |
| E2  | fixed (edit mode)       | Alt+F5 goes from line 1 to 3, 11, 22, 31 and wraps; each step announces, e.g. "Added lines 11 to 12, 2 of 4". Alt+Shift+F5 goes back. View mode stays silent (NEW-R2).                                                                                                                                             |
| E3  | fixed                   | Popup is `aria-modal`, named "Changed line 3"; Tab cycles Stage/Revert/Close; Esc closes it and puts focus back in the text; focus moving out closes it; on an unchanged line it closes and says "No git change on this line."; on an unsaved file focus starts on Close.                                          |
| E4  | fixed                   | Inline blame has `aria-hidden="true"` (active line reads `- text: // line 1`) and is `#8a8a96`: 4.9:1 on the active line. Blame gutter is 6.5:1. Toggle says "Blame shown." / "Blame hidden."; "Git: read blame of cursor line" reads the blame aloud.                                                             |
| P1  | fixed                   | The diff is tab "mod_a.rs (diff)" `[selected]`, with file tabs `tabindex=-1`, inside tabpanel "Diff of src/mod_a.rs". Esc, Alt+W and Delete on the tab close it and put focus back on the row. Alt+] goes to the first file tab, with focus on it.                                                                 |
| P2  | fixed                   | Enter on the `long.rs` row opens the diff in 8 ms and the row gets `aria-current="true"`. Esc goes back to that row; Alt+Shift+G from inside the diff focuses that row. One detail is left (NEW-R4).                                                                                                               |
| P3  | fixed                   | Panel Tab walk: message, Amend, Commit, Unstage all, Stage all, one row, Open terminal, Recent commits. Arrow keys, Home and End move through rows. S, U, O and Delete work and keep focus on the row now in place. The icon buttons are `tabindex=-1`, so the Tab count no longer grows with the number of files. |
| P4  | fixed                   | "Could not switch to other. You have changes to src/main.rs that this branch would overwrite: commit them or discard them first." (`11-switch-blocked.png`); Esc returns to the branch button.                                                                                                                     |
| P5  | regressed (multi-line)  | Ticking Amend fills in "Fix up lines", and Ctrl+Enter amends. For a commit "Subject line\n\nImportant body paragraph." the amended message is `"Subject line\n\n"`: the body is gone (NEW-R1).                                                                                                                     |
| P6  | still open (part fixed) | Git actions are in the palette now ("Git: commit", "Git: stage all changes", "Git: new branch…", "Git: switch branch…"). The clash is still there: "git show change" lists `Git: show changes` first, and "show change" + Enter turns on the tree's "Changed only" filter (NEW-R3).                                |

## New findings

### NEW-R1 major: amending a commit that has a body deletes the body

- **Area:** ui/src/git (`CommitBox.svelte`); a full fix needs `git.log`
  (crates/core protocol) or a "keep message" amend.
- **Steps:** 1. Commit "Subject line" with the body "Important body
  paragraph." (outside Ovra, or as a two-paragraph message in the box).
  2. Stage another file. 3. Alt+Shift+G, Tab, Space (Amend). 4. Shift+Tab,
  Ctrl+Enter.
- **Expected:** like `git commit --amend --no-edit`, the staged change goes
  into the last commit and the message stays the same.
- **Actual:** the box is filled with "Subject line" only. After Ctrl+Enter,
  `git log -1 --format=%B` is `"Subject line\n\n"` and the body is gone. A
  hint says "if it had more lines, add them again". Oleh does not read that
  hint before his reflex Ctrl+Enter, and a screen reader user hears it only
  as the checkbox's description. Before this round, the same keys did
  nothing ("Write a commit message"); now they quietly lose text.
- **Why it matters for Oleh:** "fix up the last commit" is his most common
  amend. Getting a lost body back means `git reflog` in a terminal.
- **Suggested fix:** if the message box holds the prefill and has not been
  edited, send an amend that keeps the existing message (`--no-edit`) and do
  not send the box text. Better still, return the full message from
  `git.log` and fill the box with all of it. Until then, do not fill the box
  at all when the last commit has a body.

### NEW-R2 minor: git change commands do nothing and say nothing in view mode

- **Area:** ui/src/editor (`commands.ts`, `git/extension.ts`)
- **Steps:** 1. Alt+P, `long.rs`, Enter. The file opens in view mode, as it
  does from the tree and the palette. 2. Press Alt+F5, or run "Git: show
  change at cursor" or "Git: read blame of cursor line".
- **Expected:** the cursor moves to the next change. If not, a message
  says "Press Ctrl+E to edit and see changes".
- **Actual:** nothing moves and nothing is announced: the observer saw only
  the palette's result counts (`VIEWMODE ALTF5 said=[]`). The view mode has
  no markers either (`30-viewmode.png`).
- **Why it matters for Iryna and Oleh:** files open in view mode by default,
  so the first Alt+F5 a user tries is silent. To them that looks like a
  broken shortcut.
- **Suggested fix:** in view mode, have the three commands switch to edit
  mode first, or announce how to get to edit mode.

### NEW-R3 minor: "show change" in the palette runs the tree filter, and the git titles still clash

- **Area:** ui/src/git (`Git.svelte` title), ui/src/editor (`commands.ts`
  title), ui/src/palette (ranking)
- **Steps:** 1. In edit mode, put the cursor on changed line 3. 2. Press
  Ctrl+Shift+P and type `show change`, then Enter. 3. Run it again with
  `git show change`.
- **Expected:** the change popup opens on line 3.
- **Actual:** step 2 lists `Show changed files only (toggle)` first, so Enter
  turns on the tree's "Changed only" filter without a word (`aria-pressed`
  true, no popup). Step 3 lists `Git: show changes Alt+Shift+G` first, and
  Enter moves focus to the commit message. The old query `show git change`
  now finds nothing. Only `change at` or `show change at` reach the popup.
- **Why it matters:** this is P6 left over. The titles still differ only by
  "s" against " at cursor", and the popup still has no key.
- **Suggested fix:** give the two commands different first words, for
  example "Git: open panel" and "Git: change at cursor (stage or revert)".
  Also give the popup a default key, as Alt+F5 now has.

### NEW-R4 minor: Esc from the staged side of a diff sends focus to the commit message, not the row

- **Area:** ui/src/git (`GitDiff.svelte`, `focus.ts`)
- **Steps:** 1. Enter on the `mod_a.rs` row under Changes (nothing of it is
  staged). 2. Click or press "Show staged changes". 3. Esc.
- **Expected:** focus goes back to the `mod_a.rs` row that opened the diff.
- **Actual:** the diff shows "No staged changes in this file." The tab says
  "mod_a.rs staged diff". Esc closes it, and focus lands in the commit
  message (`STAGED-SIDE-ESC focus=TEXTAREA`), because the close looks for
  the row in the Staged list.
- **Suggested fix:** remember the row that opened the diff, not the side
  shown last, and focus that row on close. Also consider disabling "Show
  staged changes" when the file has nothing staged.

### NEW-R5 minor: staging or reverting a hunk from the popup gives no result announcement

- **Area:** ui/src/editor (`git/extension.ts`)
- **Steps:** 1. Alt+F5 to a change. 2. "Git: show change at cursor".
  3. Enter on Stage hunk, or Tab and Enter on Revert hunk.
- **Expected:** "Hunk staged." / "Hunk reverted, Ctrl+Z undoes it.", as the
  diff view says "Hunk staged." once.
- **Actual:** staging says only "Staging..." and then the popup closes;
  `git diff --cached` shows that staging worked. Revert says nothing; the
  tab turns unsaved (`long.rs ●`).
- **Why it matters for Iryna:** she hears "Staging..." and never hears that
  it worked. After a revert she gets no hint that the change is only in the
  unsaved buffer.
- **Suggested fix:** announce the result through the editor's live region
  after the call returns, and mention Ctrl+Z after a revert.

### NEW-R6 polish: the row keys are not visible to sighted keyboard users

- **Area:** ui/src/git (`FileSection.svelte`)
- **Steps:** focus a row in Changes and look for how to stage it.
- **Actual:** the keys ("Enter: show the diff. S: stage. Delete: discard.
  O: open the file.") are only in a `hidden` description, which a screen
  reader reads, and in the icon tooltips on hover. Oleh only finds S, U and
  O by reading the code or hovering.
- **Suggested fix:** show the focused row's keys in a small line under the
  list, or in the panel's hint line, while a row has focus.

## What worked well

- Opening the panel by shortcut focused the commit message in 37 ms, even on
  the first press.
- Staging hunk 1 in the diff kept focus on the next "Stage hunk" and said
  "Hunk staged." exactly once.
- Stage all, Commit and Amend each announce once ("Staged 5 files.",
  "Committed c91fc94: …", "Amended bdda55a: …").
- The new branch field says "Branch names cannot contain spaces. Try
  my-feature." while you type, and Enter does not send the bad name.
- No sideways scrolling with the diff open at 640x400.
