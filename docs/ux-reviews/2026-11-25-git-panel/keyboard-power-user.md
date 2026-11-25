# Oleh, the keyboard power user: git panel

Build: 1395c64, headless, Chromium 158.0.8127.36, 1280x800.
Scope: Git sidebar tab (Alt+Shift+G), status, staging files and hunks, discard
and Undo, commit (Ctrl+Enter) and amend, branches, diff view in the main area,
recent commits; editor change markers, change popup ("Show git change at
cursor", Stage hunk / Revert hunk), "Toggle blame" and inline blame.
Fixtures: `small` (seeded), changed with real git per script: a committed
40-line `src/long.rs` with two changed hunks, four more modified files, extra
branches, a branch that conflicts with the dirty `src/main.rs`, and 60
untracked files for the long-list case.

## Summary

Oleh could do every git task in scope without the mouse: open the panel,
stage, discard and undo, commit, amend, create and switch branches, stage a
hunk from the diff and from the editor popup. The worst problems: the gutter
change markers are 0 px wide and never visible; the file lists cost four Tab
stops per file with no arrow keys (248 Tabs to reach the last of 62 files);
the diff view is not part of the tab model, so Alt+W closes a hidden file tab
instead of the diff, and Esc does nothing there; and the natural palette
query for the change popup runs a different command. The panel itself is
fast (open in 19-29 ms) and focus after each single action is mostly well
judged; the friction is in moving between rows, the diff and the editor.

## Findings

### KEY-1 major: gutter change markers are never visible

- **Area:** ui/src/editor (`git/extension.ts`, base theme)
- **Steps:** 1. Modify lines 3 and 30 of the committed `src/long.rs` on disk.
  2. Alt+P, `long.rs`, Enter. 3. Ctrl+E. 4. Type a new line after line 1.
- **Expected:** a coloured bar next to lines 2 (added), 4 and 31 (changed).
- **Actual:** three `[data-testid=git-marker]` elements exist with the right
  `data-change` and colour, but each is `0 x 19.5` px; Playwright reports them
  not visible and the screenshot shows no marker (`out09/01-markers.png`,
  also `03-popup.png` and `06-blame.png` in the editor run). The gutter
  element is 4 px wide with `padding: 0 2px`, which leaves a 0 px content box
  for the `display: block` marker.
- **Why it matters for Oleh:** the markers are how he sees at a glance where
  he changed a file and on which line to run "Show git change at cursor". With
  nothing visible he has to scan the file or go to the diff view.
- **Suggested fix:** give `.cm-git-marker` an explicit `width: 4px` (or drop
  the horizontal padding on `.cm-git-gutter .cm-gutterElement`), and add a
  unit/e2e check that a marker has a non-zero box.

### KEY-2 major: the palette query "show git change" runs the panel, not the popup

- **Area:** ui/src/palette (ranking) and ui/src/editor (command title)
- **Steps:** 1. Edit `src/long.rs` (Ctrl+E), cursor on line 3 (changed).
  2. Ctrl+Shift+P, type `show git change`. 3. Enter.
- **Expected:** "Show git change at cursor" is the top row (the query is a
  prefix of it), and the popup opens on line 3.
- **Actual:** the rows are `Show git changes Alt+Shift+G`, then `Show git
  change at cursor`. Enter switches the sidebar to Git and moves focus into
  the commit message (`FOCUS show change at line 3: TEXTAREA`), away from the
  editor. Only a query like `change at cursor` picks the right command.
- **Why it matters for Oleh:** the popup has no shortcut, so the palette is
  his only way to stage or revert a hunk from the editor. The obvious query
  throws him out of the editor every time, and he only notices after the
  focus jump.
- **Suggested fix:** rank an exact word-prefix match ahead of a longer title
  that merely shares the words (here "changes" vs "change"), or make the
  titles distinct, for example "Git: show changes panel" and "Git: stage or
  revert change at cursor".

### KEY-3 major: file lists are four Tab stops per file and arrow keys do nothing

- **Area:** ui/src/git (`FileSection.svelte`)
- **Steps:** 1. Alt+Shift+G (focus lands in the commit message). 2. Tab until
  the Stage button of the third changed file. 3. On a row, press ArrowDown,
  ArrowUp, Home, End, `j`. 4. Repeat step 2 with 62 changed files.
- **Expected:** one Tab stop per list; Up/Down (and Home/End) move between
  files like the file tree; a key per action on the focused row.
- **Actual:** each row is four buttons (name, ±, ↺, +), all in the Tab order.
  16 Tabs to reach "Stage src/mod_b.rs" (the third file); 248 Tabs to reach
  the last of 62 files; the panel had 254 buttons and 654 DOM nodes. Arrow
  keys, Home, End and letters leave focus where it is.
- **Why it matters for Oleh:** staging a handful of files out of a list is the
  core loop of this panel. In the file tree he already has arrows, Home/End
  and typeahead; here he is back to hammering Tab.
- **Suggested fix:** a roving tabindex per list (as in the tree): Up/Down/Home/
  End move between rows, Enter shows the diff, Space or `s` stages, `u`
  unstages, Delete or `d` asks to discard, `o` opens the file; Left/Right
  reach the row's buttons. Keep the buttons for the mouse and show the keys in
  their titles.

### KEY-4 major: Alt+W closes a hidden file tab instead of the diff; Esc does nothing in the diff

- **Area:** ui/src/git (`GitDiff.svelte`) and ui/src/editor (tabs, `editor.close_tab`)
- **Steps:** 1. Open `src/mod_a.rs` and `src/mod_b.rs` from the panel (two
  tabs). 2. In the panel, Enter on "Show diff of src/long.rs". 3. Press Esc.
  4. Press Alt+W.
- **Expected:** the diff is the active thing in the main area: Esc or Alt+W
  closes the diff and shows `mod_b.rs` again; the tab strip does not mark
  `mod_b.rs` active while the diff covers it.
- **Actual:** while the diff shows, `mod_b.rs` is still drawn as the active tab
  (`out05/01-diff-over-tabs.png`). Esc does nothing. Alt+W closes `mod_b.rs`
  (tabs `[mod_a.rs, mod_b.rs]` become `[mod_a.rs]`). In another run, with only
  `long.rs` open and its own diff shown, Alt+W closed the `long.rs` tab and
  left the diff on screen, so nothing visible changed
  (`out04/03-after-hunk.png` vs `05-closed.png`).
- **Why it matters for Oleh:** Mod+W and Esc are his "close what I am looking
  at" keys. Here they either do nothing or close something he cannot see. With
  an unsaved buffer underneath this would lead into a close prompt for a file
  he was not looking at.
- **Suggested fix:** while `workspace.gitDiff` is set, have Mod+W and Esc
  close the diff first (and return focus to the panel row that opened it), and
  show no tab as active, or open the diff as its own tab.

### KEY-5 major: reviewing several diffs loses your place in the list every time

- **Area:** ui/src/git (`GitPanel.svelte`, `GitDiff.svelte`, `Git.svelte`)
- **Steps:** 1. Alt+Shift+G. 2. Enter on the name `long.rs`. 3. Tab to its ±,
  Enter. 4. Stage a hunk, then press Alt+Shift+G to go back to the list.
  5. Press "Close diff".
- **Expected:** the spec says "Selecting a file shows its diff", so Enter on
  the row shows the diff; after a diff, one key takes him back to the row he
  came from, and closing the diff returns focus there.
- **Actual:** Enter on the name opens the file in the viewer, not the diff, and
  keeps focus on the row (`out04/01-enter-name.png`). The diff only opens from
  the ± button, which moves focus to the diff heading. Alt+Shift+G always
  lands in the commit message, never on the row. "Close diff" puts focus on
  the empty main view (`DIV.view aria=No file open`). Reviewing file _i_
  costs Alt+Shift+G plus 6 + 4(i-1) Tabs to its ±, then 4 more Tabs in the
  diff to reach "Stage hunk".
- **Why it matters for Oleh:** "look at each change, stage the good hunks" is
  the main use of the panel, and every file restarts the walk from the top.
- **Suggested fix:** Enter on a row shows the diff (keep "Open file" in the
  diff header and on `o`); remember the last row and focus it when the panel
  is shown again by its shortcut if focus was in the diff; Close diff and Esc
  return focus to that row. Consider `n`/`p` (or Alt+Down/Up) in the diff to
  go to the next file's diff.

### KEY-6 major: amend cannot keep the last message

- **Area:** ui/src/git (`CommitBox.svelte`)
- **Steps:** 1. Commit "Add extra line to c" with Ctrl+Enter. 2. Stage another
  file. 3. Alt+Shift+G, Tab to "Amend last commit", Space. 4. Shift+Tab back,
  Ctrl+Enter with the message empty.
- **Expected:** like `git commit --amend --no-edit`: the staged change is
  folded into the last commit with its message, or the box is prefilled with
  the last message when Amend is checked.
- **Actual:** nothing happens; the hint says "Write a commit message." The box
  stays empty after checking Amend, so the old message must be retyped. The
  recent commits list cannot be focused, so the message cannot even be copied
  from there with the keyboard. Amend with a typed message works.
- **Why it matters for Oleh:** "fix up the last commit" is one of his most
  frequent git actions, and retyping a message is exactly the kind of extra
  work he notices.
- **Suggested fix:** when Amend is checked and the box is empty, fill it with
  the last commit's full message (selected, so typing replaces it), and drop
  the prefill when Amend is unchecked unless edited.

### KEY-7 minor: focus goes to an invisible heading after staging the last file or Unstage all

- **Area:** ui/src/git (`FileSection.svelte`, `GitPanel.svelte`)
- **Steps:** 1. Alt+Shift+G. 2. Stage `src/main.rs`, then `src/new_module.rs`
  (the last change) with Enter. 3. Or: Enter on "Unstage all".
- **Expected:** after the last change is staged, focus goes to the commit
  message (the obvious next step); after Unstage all, to the first row of
  Changes.
- **Actual:** focus is on the visually hidden `h2` "Git"
  (`FOCUS after staging last: H2.visually-hidden`); nothing on screen shows
  focus (`out11/01-last-staged.png`). The next Tab goes to the branch button.
- **Why it matters for Oleh:** after every action he expects to see where he
  is; here he has to press a key to find out.
- **Suggested fix:** in `onfocuslost`, prefer the commit message when there are
  staged files, else the first row of the other list; the hidden heading only
  as a last resort.

### KEY-8 minor: git actions are not in the command palette

- **Area:** ui/src/git (command registration)
- **Steps:** Ctrl+Shift+P, type `commit`, then `branch`, then `stage`.
- **Expected:** commands like "Git: commit", "Git: stage all", "Git: switch
  branch…", "Git: new branch…", "Git: amend last commit".
- **Actual:** `commit` and `branch` match nothing; `stage` lists "Stop the
  agent session", "Show git changes" and "Show git change at cursor".
- **Why it matters for Oleh:** his habit is "Mod+Shift+P for any command". The
  panel works, but only after Alt+Shift+G and Tab walking.
- **Suggested fix:** register palette commands that focus the right control
  (switch branch opens the branch menu, commit focuses the message, stage all
  runs it).

### KEY-9 minor: branch menu loses focus on Cancel and has no arrow keys or filter

- **Area:** ui/src/git (`Branches.svelte`)
- **Steps:** 1. Alt+Shift+G, Shift+Tab, Enter (menu opens, focus on the current
  branch). 2. ArrowDown, `f`. 3. "New branch…", Tab, Tab to Cancel, Enter.
  4. Esc.
- **Expected:** arrows and typeahead move through branches; Cancel returns
  focus to "New branch…"; Esc then closes the menu.
- **Actual:** ArrowDown and `f` do nothing (Tab only). Cancel drops focus to
  `BODY`; Esc then does nothing and the menu stays open. Esc inside the name
  input does close the menu and focus the branch button, which is good.
- **Why it matters for Oleh:** with 20+ branches, switching is a Tab marathon;
  a lost focus after Cancel means reaching for Alt+Shift+G and Shift+Tab again.
- **Suggested fix:** focus "New branch…" after Cancel; make the list a
  listbox with Up/Down/Home/End and typeahead, or put a filter field on top
  (Enter switches to the top match, or offers "Create <name>").

### KEY-10 minor: the change popup lets focus escape and then ignores Esc

- **Area:** ui/src/editor (`git/extension.ts`)
- **Steps:** 1. Editor on a changed line, run "Show git change at cursor"
  (focus on Stage hunk, good). 2. Tab three times. 3. Esc.
- **Expected:** Tab cycles Stage hunk, Revert hunk, Close; Esc closes the
  popup and returns to the editor.
- **Actual:** the third Tab leaves the popup for "Hide" in the Approvals panel;
  Esc there does nothing and the popup stays open over the code
  (`popup after esc: 1`).
- **Why it matters for Oleh:** a dialog he cannot close with Esc once he
  tabbed one key too far breaks his "Esc closes anything" rule.
- **Suggested fix:** wrap Tab inside the popup (it has `role=dialog`), and
  close it when focus leaves it.

### KEY-11 minor: several git keys do nothing, silently

- **Area:** ui/src/editor, ui/src/git
- **Steps:** 1. "Show git change at cursor" on an unchanged line, or with the
  file open in view mode. 2. Ctrl+Enter in the commit message when it is
  empty, or with focus on the Amend checkbox or the Commit button.
  3. Alt+Shift+G before a project is open.
- **Expected:** a short status or announcement: "No git change on this line",
  "Write a commit message", "Open a project first"; Ctrl+Enter commits from
  anywhere in the commit group.
- **Actual:** nothing happens and focus does not move. Only for the empty
  message is the reason visible, as the hint under the button.
- **Why it matters for Oleh:** he "notices every key that silently does
  nothing" and then cannot tell a bug from a no-op.
- **Suggested fix:** announce the reason through the panel's live region or a
  short editor status; handle Ctrl+Enter on the whole commit group.

### KEY-12 minor: no shortcut for the change popup or blame, and no next/previous change

- **Area:** ui/src/editor (`commands.ts`)
- **Steps:** In edit mode, try to go to the next change (F7, Alt+F5,
  Alt+ArrowDown) or stage the hunk at the cursor without the palette.
- **Expected:** a way to jump between changes (Vim's `]c`/`[c`, VS Code's
  Alt+F5) and a key for "Show git change at cursor".
- **Actual:** none of these move to a change; "Show git change at cursor" and
  "Toggle blame" have no keys (the code comment says free browser-safe keys
  are scarce), so each hunk costs Ctrl+Shift+P, about 15 characters and Enter.
- **Why it matters for Oleh:** going hunk by hunk is a keyboard flow by nature.
- **Suggested fix:** add "Go to next/previous git change" commands, and give
  the popup a key even if only a stand-in in the browser; a palette that
  remembers the last command would also help.

### KEY-13 minor: "Reveal open file in the tree" does nothing from the Git tab

- **Area:** ui/src/tree (or ui/src/shell for a "Show files" command)
- **Steps:** 1. Alt+Shift+G, Enter on `main.rs` (opens it). 2. Ctrl+Shift+P,
  "Reveal open file in the tree", Enter.
- **Expected:** the sidebar switches to Files and the file is revealed and
  focused.
- **Actual:** the sidebar stays on Git, focus stays on the row; there is also
  no command or shortcut to go back to the Files view (palette `files` lists
  only "Search in files" and "Show changed files only").
- **Why it matters for Oleh:** after the Git tab replaces the tree, he has no
  keyboard way back except Tab to the "Files" tab button.
- **Suggested fix:** have `tree.reveal` set `workspace.sidebar = "files"`
  first, and add a "Show files" command with a shortcut.

### KEY-14 polish: inline blame text is hard to read

- **Area:** ui/src/editor (`git/blame-view.ts`)
- **Steps:** Edit `src/long.rs`, rest the cursor on line 1.
- **Expected:** at least 4.5:1, or 3:1 for this deliberately dim hint.
- **Actual:** `rgb(90,90,102)` on `rgb(29,29,35)`, italic 13 px: about 2.5:1.
- **Suggested fix:** use the hint colour of the panels (`#9a9aa6`, about 6:1)
  or something in between.

### KEY-15 polish: duplicated prefix in the switch error

- **Area:** ui/src/git (`model.svelte.ts`) or crates/worktree (message)
- **Steps:** with `src/main.rs` dirty, switch to a branch that changes it.
- **Actual:** "Could not switch to other: cannot switch to other: error: Your
  local changes to the following files would be overwritten by checkout: ..."
  (`out10/02-switch-failed.png`). Focus correctly stays on the branch.
- **Suggested fix:** show one prefix and git's message, and suggest the next
  step ("Commit or discard src/main.rs first").

## What worked well

- Alt+Shift+G opens the panel in 19-29 ms and lands in the commit message;
  the palette shows the shortcut next to "Show git changes".
- After staging or discarding a row, focus moves to the same button on the row
  now in its place, so repeated Enter stages down the list.
- Discard asks inside the row and starts on Cancel; Esc cancels and returns to
  the discard button; Undo restores the file and focuses its row.
- Ctrl+Enter commits and keeps focus in the cleared message; the result line
  ("Committed f0440b7: ...") and the hook output are right there. Amend works
  by keyboard (Tab, Space, Shift+Tab, Ctrl+Enter).
- Branch errors (bad name, existing name, a switch that would overwrite
  changes) appear in the menu with focus kept where it was; creating a branch
  switches to it and returns focus to the branch button.
- In the diff, "Stage hunk" keeps focus on the next hunk's button; hunk buttons
  have useful labels ("Stage hunk 2, lines 27 to 33").
- The change popup focuses "Stage hunk", explains why it is disabled for an
  unsaved file, and Esc from inside it returns to the editor; "Revert hunk" is
  undone with Ctrl+Z.
