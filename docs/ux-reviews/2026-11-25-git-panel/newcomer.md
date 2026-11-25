# Nadia, the newcomer: git panel

Build: 1395c64, headless, Chromium 158.0.8127.36, 1280x800.
Scope: the git panel (sidebar tab "Git", Alt+Shift+G): status, staging files
and hunks, discard and Undo, commit and amend, branches, the diff view,
recent commits; in the editor: change markers in the gutter, the change popup
(Stage hunk / Revert hunk), "Toggle blame" and inline blame.
Fixtures: `small`, `empty` (changed with real git inside the scripts).

## Summary

Nadia could do the everyday job from the panel without help: she saw what
changed, staged a whole file and a single hunk, committed, amended, created a
branch and got a deleted file back with Undo. The worst problem is in the
editor: the change markers in the gutter have no width, so she never sees
them and does not know there is a popup to click. Clicking a file in the
panel opens a plain viewer instead of the diff she knows from VS Code, and
raw git errors (including "stash", which Ovra does not have) leave her
stuck. The panel itself feels calm and readable.

## Findings

### NEW-1 major: change markers in the editor gutter cannot be seen

- **Area:** ui/src/editor (`editor/git/extension.ts`, the `styles` base theme)
- **Steps:** 1. Change lines 3 and 11 to 12 of a 30-line committed file and
  add two lines after line 20 (real git, outside Ovra). 2. Git tab, click
  `long.rs`, click "Edit". 3. Look at the gutter between the line numbers
  and the text.
- **Expected:** coloured bars (changed, added) and a red "deleted" mark next
  to lines 3, 11 and 21 to 22, as in VS Code.
- **Actual:** nothing is visible (`31-edit.png`, zoom `31b-gutter-zoom.png`).
  The three markers are in the DOM (`aria-label` "Changed lines, click for
  actions" and so on), but each marker measures 0 px wide: the gutter
  element is `width: 4px; padding: 0 2px` with CodeMirror's
  `box-sizing: border-box`, so the content box is 0 px (probe: gutter
  element `[4, "border-box", "2px", "4px"]`, marker width `0`, background
  `rgb(79, 143, 216)`). A click on that 4 px strip does open the popup,
  but nothing tells her it is there.
- **Why it matters for Nadia:** the markers are the only way she would ever
  find "Stage hunk", "Revert hunk" and the "In the index" preview from the
  editor. The whole editor half of the feature is invisible to a mouse user.
- **Suggested fix:** drop the padding or set `box-sizing: content-box` on
  `.cm-git-gutter .cm-gutterElement` (or give the marker its own width, for
  example 3 px, with a few px of clickable margin); add a test that checks
  the marker's rendered width is not 0.

### NEW-2 major: clicking a file in the git panel opens the file, not its diff

- **Area:** ui/src/git (`FileSection.svelte`, `open()`)
- **Steps:** 1. Git tab. 2. Click the `main.rs` row under "Changes".
- **Expected:** the diff of `main.rs` in the main area (the spec: "Selecting
  a file shows its diff in the main area"; VS Code does the same).
- **Actual:** the file opens in the read-only viewer, with no sign of what
  changed and no change markers (`11-row-click.png`, `51-open-from-diff.png`:
  0 markers in view mode). The diff is only behind the small "±" button,
  whose meaning is in a tooltip only. "Open file" in the diff view also
  closes the diff and opens the viewer, again without markers.
- **Why it matters for Nadia:** she clicks the name, sees the same code she
  wrote, and concludes that Ovra cannot show diffs. She only found the diff
  by hovering the icons one by one.
- **Suggested fix:** make the row click show the diff (as the spec says) and
  keep "Open file" as the explicit action, or at least open the file in
  edit mode so the gutter markers show. Mark the row whose diff is open as
  selected (`aria-current`) so she can see which file the diff belongs to.

### NEW-3 major: a blocked branch switch shows raw git output and suggests "stash", which Ovra does not have

- **Area:** ui/src/git (`model.svelte.ts`, `switchTo`) and crates/core
  (`src/git.rs`, the error text)
- **Steps:** 1. Create `feature/login` from the branch menu. 2. Commit a
  change to `README.md` there, then edit `README.md` again without
  committing. 3. Branch menu, click `main`.
- **Expected:** a plain message that names the file and says what she can do
  in Ovra: commit it, or discard it (with Undo), then switch.
- **Actual:** "Could not switch to main: cannot switch to main: error: Your
  local changes to the following files would be overwritten by checkout:
  README.md Please commit your changes or stash them before you switch
  branches. Aborting" (`44-switch-blocked.png`). The prefix is repeated,
  git's line breaks are lost, and "stash" is not something the panel offers.
- **Why it matters for Nadia:** she reads every message literally, searches
  the panel for "stash" and finds nothing. She does not know whether
  "Aborting" means something broke.
- **Suggested fix:** map this case (the core already knows it is a refused
  switch) to its own message, e.g. "Switching would overwrite your changes
  to README.md. Commit or discard them first." with the file names as a
  list; keep git's raw text in a collapsed "Details" like the commit hook
  output. Do not repeat "cannot switch to main".

### NEW-4 minor: the diff in the main area keeps the previous file's tab selected

- **Area:** ui/src/shell (main area) with ui/src/git (`GitDiff.svelte`)
- **Steps:** 1. Click `main.rs` in the panel (it opens in a tab). 2. Click
  "±" next to `long.rs`.
- **Expected:** a tab or title that says "long.rs (diff)", or no file tab
  highlighted.
- **Actual:** the tab bar still shows `main.rs` as the selected tab
  (`aria-selected="true"`) while the content is the diff of `src/long.rs`
  (`12-diff.png`). Clicking the `main.rs` tab does not obviously "go back".
- **Why it matters for Nadia:** two different file names on screen at once;
  she is not sure which file she is staging hunks of.
- **Suggested fix:** show the diff as its own tab (or deselect file tabs
  while a diff is shown), titled with the file name and "diff".

### NEW-5 minor: "Push and pull run in your terminal" opens an empty terminal

- **Area:** ui/src/git (`GitPanel.svelte`, `openTerminal`)
- **Steps:** 1. Git tab. 2. Under "Push and pull run in your terminal.",
  click "Open terminal".
- **Expected:** per the spec, a terminal with the command typed but not run
  (for example `git push`), so she only has to press Enter.
- **Actual:** a new terminal with an empty prompt (`64-terminal.png`, the
  same button in the "not a git repository" state).
- **Why it matters for Nadia:** she does not remember whether it is
  `git push`, `git push origin main` or `git push -u origin feature/login`;
  the panel sent her away with no help.
- **Suggested fix:** offer "Push" and "Pull" buttons that open the terminal
  with the right command typed (with `-u origin <branch>` when there is no
  upstream), as the spec describes.

### NEW-6 minor: the stage, discard and diff buttons are small symbols with tooltips only

- **Area:** ui/src/git (`FileSection.svelte`)
- **Steps:** 1. Git tab. 2. Look at the three buttons on each row.
- **Expected:** recognisable actions, or labels on hover-free screens.
- **Actual:** "±", "↺" and "+" (22x17 px each) explain themselves only in a
  `title` tooltip. "↺" (discard) sits between "show diff" and "stage", so a
  slip of a few pixels hits it; the in-row confirm saves her, which is good.
- **Why it matters for Nadia:** she hovered each one to learn what it does,
  and "±" for "show diff" is not something she would guess.
- **Suggested fix:** bigger targets (at least 24x24), clearer icons (a diff
  icon, a "discard" icon used by other editors), and a visible text label
  on the selected or hovered row.

### NEW-7 minor: amend starts with an empty message

- **Area:** ui/src/git (`CommitBox.svelte`)
- **Steps:** 1. Commit "Say hello world". 2. Tick "Amend last commit" to fix
  the comma.
- **Expected:** the last commit's message filled in, ready to edit (VS Code
  and `git commit --amend` both start from it).
- **Actual:** the box stays empty and "Amend commit" waits with "Write a
  commit message." (`41-amend.png`). To add a forgotten file and keep the
  message she must retype it from memory.
- **Suggested fix:** when "Amend" is ticked and the box is empty, fill it with
  the last commit's full message (the log already has the summary; the body
  needs a field or `git.log` detail); restore what she typed if she unticks.

### NEW-8 minor: an invalid branch name is refused without saying why

- **Area:** ui/src/git (`Branches.svelte`) and crates/core (`src/git.rs`)
- **Steps:** 1. Branch menu, "New branch…". 2. Type `my feature`, "Create
  and switch".
- **Expected:** "Branch names cannot contain spaces. Try my-feature." and
  ideally the hint before she presses the button.
- **Actual:** 'Could not create my feature: "my feature" is not a valid branch
  name' (`43-branch-space.png`).
- **Suggested fix:** say which rule failed (spaces, `..`, leading `-`, and so
  on) and suggest a fixed name; or replace spaces with `-` as she types.

### NEW-9 polish: git words where plain ones would do

- **Area:** ui/src/editor (`editor/git/extension.ts`), ui/src/git
  (`GitDiff.svelte`, `CommitBox.svelte`)
- **Steps:** open the change popup; open any diff; open an empty repository.
- **Actual:** the popup says "In the index:" and "New lines, not in the
  index." (`32-popup.png`); hunks are titled `@@ -1,6 +1,6 @@`; in a
  repository with no commits the hint still says "…or amend the last
  commit." though there is no commit and no Amend box (`60-empty-repo.png`).
- **Suggested fix:** "Before your change:" / "New lines."; a hunk title like
  "Lines 1 to 6" with the `@@` header in a tooltip; drop the amend clause
  when `status.head` is null.

### NEW-10 polish: commit results and blame details that look stale or odd

- **Area:** ui/src/git (`CommitBox.svelte`), ui/src/editor
  (`editor/git/blame-view.ts`)
- **Steps:** 1. Amend a commit. 2. Switch branch. 3. Toggle blame on a file
  that ends with a newline.
- **Actual:** after amending, the visible line still says "Committed
  829c10d" (only the hidden announcement says "Amended"); the "Committed …"
  line and "Output of git and hooks" stay after switching to another
  branch (`44-switch-blocked.png`); the empty last line of the file gets
  "Not committed yet" in the blame gutter (`39-blame.png`); the inline blame
  is grey italic like comments, so in a commented file it reads as code.
  Clicking a commit under "Recent commits" does nothing and gives no hint.
- **Suggested fix:** say "Amended …" when it was an amend; clear the result
  line on the next action or branch switch; skip blame on the empty last
  line; give inline blame its own colour or a separator; make it clear
  recent commits are a list only (or show the commit on click later).

## What worked well

- The panel loaded in about 30 ms and reads top to bottom like a checklist:
  branch, message, staged, changes, push/pull, recent commits.
- Commit says why it waits ("Stage changes to commit them…", "Write a commit
  message."), and Ctrl+Enter is shown in the placeholder; plain Enter makes
  a new line, as she expected.
- Discard asks first, inside the row, with focus on Cancel, says plainly what
  happens ("The file is deleted. You can undo this until the next
  discard."), and Undo really brought the untracked file back.
- Per-hunk staging in the diff view worked on the first try; the staged file
  appeared at once under "Staged changes" and the diff narrowed to the rest.
- Saving is required before "Stage hunk" in the editor, and the popup says so
  ("Save the file to stage this change.").
- A folder that is not a repository gets a clear explanation with "Open
  terminal" and "Check again"; an empty repository can make its first
  commit from the panel.
- "Toggle blame" is easy to find by typing "blame" in the palette, and the
  palette search "git" shows the panel command with its Alt+Shift+G key.
