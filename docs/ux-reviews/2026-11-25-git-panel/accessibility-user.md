# Iryna (accessibility user): git panel (2026-11-25)

Build: 1395c64 on main, headless (scripted fake agent), Playwright Chromium
153.0.8010.12, 1280x800. Also 640x400 and 512x384 CSS px (200% zoom of a 1280 px
and a 1024 px window), forced colours on and off.
Scope: the git panel (sidebar tab "Git", Alt+Shift+G): status lists, staging
files and hunks, discard and Undo, commit and amend, a failing pre-commit hook,
the branch menu, the diff view in the main area and how it relates to the
selected tab and the tabpanel, recent commits, a merge conflict; in the
editor: change markers in the gutter, the change popup ("Show git change at
cursor"), "Toggle blame" and inline blame. Fixtures: `small` (seeded), changed
with real git in the scripts: a 30-line `src/lib.rs` committed and then edited
in two places (two hunks), or with lines added, changed and deleted; a
`pre-commit` hook that exits 1; a merge of two branches that both edit
`README.md`.

## Summary

With the keyboard and a screen reader, I could do every task in the panel
itself: stage files and single hunks, discard and undo, commit, amend,
create a branch, read a hook error. Names, live announcements and focus after
each action are better than in any earlier panel. The editor side is a
different story. The change markers are 0 px wide, so nobody can see them.
The gutter is `aria-hidden`, so a screen reader cannot find changes either.
Inline blame is part of the code line's text, at 2.5:1 contrast. The change
popup lets focus leave without closing. While the diff view is open, the tab
strip still says "README.md, selected".

## Findings

### A11Y-36 major: change markers in the editor gutter are 0 px wide, so no one can see them

- **Area:** ui/src/editor (git/extension.ts, styles)
- **Steps:** 1. In `demo-app`, commit a 30-line `src/lib.rs`, then add two
  lines after line 5, change line 12 and delete line 20. 2. Open
  `src/lib.rs` (Alt+P) and press Ctrl+E to edit. 3. Look at the space between
  the line numbers and the text.
- **Expected:** a green bar at the added lines, a blue bar at line 14 and a
  red tick at the deletion, as the spec says ("gutter markers (added, changed,
  deleted)").
- **Actual:** there is nothing to see (`05-editor.png`, `05b-popup.png`, and
  the gutter crop `gutter.png`). The markers are in the DOM, but their boxes
  have no width: `cm-gutterElement 4.0x19.5`, `cm-git-marker cm-git-added
  0.0x19.5 w=0px`, `cm-git-modified 0.0x19.5`, `cm-git-deleted 0.0x3.0`.
  The gutter cell is `width: 4px` with `padding: 0 2px`, so the content box
  that holds the block-level marker is 0 px wide.
- **Why it matters for Iryna:** at 200% zoom she looks for colour bars to
  find what she changed. They are not drawn, and the next two findings
  remove the other routes to that information.
- **Suggested fix:** give `.cm-git-marker` an explicit width (for example
  `width: 3px` with `box-sizing: content-box`, or make the cell
  `padding: 0`) and draw it with a border or an outline as well as a
  background, so it also shows in forced colours (A11Y-41). Add a unit or
  e2e check that a marker's bounding box is wider than 0.

### A11Y-37 major: a screen reader or keyboard user cannot find the changed lines in the editor

- **Area:** ui/src/editor (git/extension.ts, commands.ts)
- **Steps:** 1. Same file as A11Y-36, in edit mode. 2. Try to get from the
  editor to a marker, or to hear where the changes are. 3. Open the palette
  (Ctrl+Shift+P) and look for a way to go to the next change.
- **Expected:** a keyboard way to move to the next and previous change (most
  editors have "Go to next change", e.g. Alt+F5 or F7), which announces
  "Changed lines 14", plus a default shortcut for "Show git change at
  cursor". The markers' labels should reach assistive technology somewhere.
- **Actual:** the markers carry `role="img"` and labels such as "Changed
  lines, click for actions", but they sit inside `.cm-gutters`, which has
  `aria-hidden="true"` (`ariaHiddenAncestor=true tabindex=-1` for all three).
  Tab inside the editor inserts a tab character (line 1 got indented);
  Escape then Tab leaves the editor for the Approvals panel. The only
  keyboard route is the palette command "Show git change at cursor", which
  has no shortcut and works only when the cursor is already on a changed
  line. On a line with no change it does nothing and says nothing (see
  A11Y-44). There is no "next change" command.
- **Why it matters for Iryna:** she has to guess which line changed, or
  switch to the diff view and map hunk line numbers back by hand. That is a
  workaround for every gutter action the spec promises ("stage a hunk from
  the gutter").
- **Suggested fix:** add "Go to next git change" and "Go to previous git
  change" commands that move the cursor to the next hunk, announce its kind
  and range through a live region ("Changed lines 14 to 14, 2 of 3"), and
  can open the popup. Give "Show git change at cursor" a default shortcut
  that browsers leave alone. Phase plan permitting, list the counts in the
  editor's status ("3 changes").

### A11Y-38 major: inline blame becomes part of the code line for a screen reader, and is unreadable at 2.5:1

- **Area:** ui/src/editor (git/blame-view.ts)
- **Steps:** 1. Open any committed file in edit mode (here `src/lib.rs`, or
  `README.md` during a merge conflict). 2. Leave the cursor on a line for
  half a second. 3. Read the line with a screen reader (line text of
  `.cm-activeLine`), and measure the contrast of the blame.
- **Expected:** blame that a screen reader user can ask for (a command
  that announces it), never mixed into the source text. Text contrast of at
  least 4.5:1.
- **Actual:** the widget is inside the `contenteditable` line with no
  `aria-hidden`, so the line reads `"// line 4Iryna, just now · lib: thirty
  lines"`, and on a changed line `"// line 12 changedNot committed yet"`. On
  a conflict file the editor's text starts with
  `"<<<<<<< HEADNot committed yet# Demo app (main)"`. It moves with the
  cursor, so every arrow key can change what the line reads as. The colour
  is `#5a5a66` on `#1d1d23`: 2.47:1 (`04b-inline-blame.png`). The blame
  gutter after "Toggle blame" is `#6f6f7a` on `#16161a`: 3.64:1
  (`04c-blame-gutter.png`), and it is inside the `aria-hidden` gutters, so
  "Toggle blame" changes nothing a screen reader can reach and announces
  nothing.
- **Why it matters for Iryna:** NVDA reads the author and commit summary as
  if they were code, with no space between them. She cannot tell where the
  code ends, and with low vision she cannot read the dimmed text either.
- **Suggested fix:** set `aria-hidden="true"` on the inline widget. Add a
  command "Show blame of this line" that announces "Iryna, 3 days ago: lib:
  thirty lines" through a polite live region, and a setting or a command to
  turn the inline blame off. Raise the inline and gutter blame colours to
  at least 4.5:1 (for example `#9a9aa6`, 6.5:1 on `#16161a`). Have "Toggle
  blame" announce "Blame shown" or "Blame hidden".

### A11Y-39 major: while the diff view shows, the tab strip still says another file is selected

- **Area:** ui/src/shell (Shell.svelte), ui/src/editor (Tabs.svelte), ui/src/git (GitDiff.svelte)
- **Steps:** 1. Open `README.md` (Alt+P). 2. Press Alt+Shift+G, Tab to
  "Show diff of src/lib.rs", press Enter. 3. Look at the accessibility tree
  of `main`.
- **Expected:** the visible content and the selected tab agree. Either the
  diff is a tab of its own ("src/lib.rs (diff)", selected, controlling the
  panel that shows it), or no tab is selected while the diff shows, or the
  diff renders inside the tabpanel and the tabpanel's name says so.
- **Actual:** `tab "README.md" [selected]` with
  `aria-controls="ovra-tabpanel"`, while `#ovra-tabpanel` is `hidden` and
  the diff is a separate `region "src/lib.rs Unstaged changes"` next to the
  tablist (`03-diff.png`, ARIA-MAIN in the log). Arrowing to the tab and
  pressing Enter closes the diff without a word. Alt+] (next tab) also
  closes it, and focus falls to `<body>` (A11Y-40).
- **Why it matters for Iryna:** the screen reader says she is on README.md,
  but what is shown is the diff of another file. Tab-strip navigation, her
  usual way to know where she is in the main area, gives the wrong answer.
- **Suggested fix:** treat the diff as a tab: add a tab entry for it while
  it is open (with a close button), select it, and render the diff in the
  `role="tabpanel"` with `aria-labelledby` set to that tab. If that is too
  much for now, set `aria-selected="false"` on every file tab while
  `workspace.gitDiff` is set and give the diff region an
  `aria-roledescription` or a heading that says "Diff".

### A11Y-40 major: switching tabs from the diff view drops focus to the page

- **Area:** ui/src/editor (commands.ts, next/previous tab), ui/src/git (GitDiff.svelte)
- **Steps:** 1. With one tab (`README.md`) open, open the diff of
  `src/lib.rs` from the git panel. 2. Stage hunk 1 (focus stays on the
  next "Stage hunk", good). 3. Press Alt+] (the browser stand-in for
  Ctrl+Tab).
- **Expected:** the diff closes, the README tab shows, and focus goes to the
  selected tab or into the tabpanel.
- **Actual:** the diff closes and `document.activeElement` is `<body>`
  (`AFTER-NEXT-TAB diffVisible=false focus=body`). The next Tab starts again
  from the header.
- **Why it matters for Iryna:** this is the same focus loss as A11Y-27 in a
  new place. With a screen reader she hears nothing and has to find her way
  back from the top of the page.
- **Suggested fix:** when a tab command or a tab click hides the diff, move
  focus to the selected tab, as Close diff does with the tabpanel. A shared
  "focus after main-area change" helper in the shell would cover both.

### A11Y-41 major: the change popup is a dialog that focus can leave, and then it stays open

- **Area:** ui/src/editor (git/extension.ts, popupTooltip/popupDom)
- **Steps:** 1. Put the cursor on changed line 14 and run "Show git change at
  cursor" from the palette. Focus is on "Stage hunk", good. 2. Press Tab
  three times. 3. Press Escape. 4. Move to line 1, which has no change, and
  run the command again.
- **Expected:** focus stays in the popup (Tab cycles through Stage hunk,
  Revert hunk, Close), or the popup closes when focus leaves it. Escape
  closes it from anywhere.
- **Actual:** Tab goes from Close to the Approvals panel's "Hide", then
  "Check again" (`POPUP-TAB3/4`). Escape there does nothing and the popup
  is still on screen (`POPUP-ESC ... count=1`). Running the command on line
  1 leaves the old popup for line 14 open (`NO-CHANGE popup=1`). It has
  `role="dialog"` but no `aria-modal`, no focus trap, and its Escape handler
  works only when focus is inside it.
- **Why it matters for Iryna:** a dialog that focus walks out of is
  disorienting. At 200% zoom the popup also covers four lines of code that
  she can no longer reach with the mouse without first finding the Close
  button again.
- **Suggested fix:** close the popup on `focusout` when the new focus is
  outside it (and return focus to the editor), or trap Tab inside it like
  the other dialogs. Close it when "Show git change at cursor" finds no
  change at the cursor.

### A11Y-42 minor: "Added" and "changed" markers differ only by colour, and look the same in forced colours

- **Area:** ui/src/editor (git/extension.ts styles)
- **Steps:** 1. Same file as A11Y-36. 2. Compare the added and changed
  markers' styles. 3. Emulate `forced-colors: active`.
- **Expected:** the kinds differ by shape as well (for example a solid bar
  for added, a dashed or striped one for changed, a triangle for deleted).
  In forced colours each stays visible with `forced-color-adjust: none` and a
  system colour.
- **Actual:** added is `rgb(76,175,80)`, changed is `rgb(79,143,216)`, both a
  plain bar. With forced colours both become `rgb(255,255,255)`
  (`FORCED-MARKERS`, `04f-forced-colors.png`), so they cannot be told apart,
  even once A11Y-36 is fixed.
- **Why it matters for Iryna:** she uses forced colours on bad days, and
  telling green from blue at low vision is not reliable.
- **Suggested fix:** use different shapes, and add a
  `@media (forced-colors: active)` rule that draws the markers with
  `CanvasText`/`Highlight` borders.

### A11Y-43 minor: text below 4.5:1 in the diff view's line numbers

- **Area:** ui/src/git (GitDiff.svelte)
- **Steps:** 1. Open the diff of `src/lib.rs`. 2. Measure the line-number
  columns.
- **Expected:** at least 4.5:1, as the code text has (10.9 to 14.5:1).
- **Actual:** `#6f6f7a` on `#16161a`: 3.64:1. On a removed line's
  background (`#3a1f22`): 3.02:1. The numbers are `aria-hidden`, so the
  only way to know which line a hunk touches is to see them, or to rely on
  the hunk's label ("lines 9 to 15").
- **Why it matters for Iryna:** she maps diff lines to the editor by these
  numbers, at 200% zoom.
- **Suggested fix:** use `#9a9aa6` (6.5:1 on the base, about 5.4:1 on the
  removed background) for `.num`.

### A11Y-44 minor: some actions give no spoken result, and others give it twice

- **Area:** ui/src/git (CommitBox.svelte, GitDiff.svelte, model), ui/src/editor (git/extension.ts)
- **Steps:** 1. Commit something, so the panel's status region says
  "Committed cf21d48: Say hello world". 2. Type a new message with nothing
  staged and press Ctrl+Enter. 3. In the editor, run "Show git change at
  cursor" on an unchanged line. 4. On a changed line, press Stage hunk in the
  popup. 5. In the diff view, press Stage hunk.
- **Expected:** every keyboard action has one spoken result: "Nothing is
  staged: stage changes to commit them" in step 2, "No git change on this
  line" in step 3, "Hunk staged" in step 4, and one "Hunk staged" in step 5.
- **Actual:** step 2 does nothing, and the status region still holds the
  old "Committed cf21d48" text (`EMPTY-COMMIT`). The reason is only in
  `aria-describedby` of the Commit button, which has no focus. Step 3 does
  nothing (`live=[...]` unchanged). Step 4 closes the popup and returns
  focus to the editor without any announcement, although the index changed
  (`STAGED-FROM-POPUP`). Step 5 sets "Hunk staged." in two status regions
  at once, the diff's and the panel's (`diffstatus="Hunk staged."
  panel="Hunk staged."`), so NVDA can read it twice. A branch-name error and
  a hook failure are also both in the status region and in a
  `role="alert"`.
- **Why it matters for Iryna:** silence after Ctrl+Enter, with the old
  "Committed" still there, reads as success. Double readings slow down a
  repeated action like staging hunk after hunk.
- **Suggested fix:** in `commit()`, when `why` is set, write it to the
  model's notice. Announce "No git change on this line" and "Hunk staged" from
  the editor through the shared notifications live region. Keep one owner
  per message: let the diff view use the model's notice instead of its own
  status, and do not repeat an alert's text in the status region.

### A11Y-45 minor: focus does not go back to the panel after Close diff, and Escape does not close the diff

- **Area:** ui/src/git (GitDiff.svelte)
- **Steps:** 1. In the panel, focus "Show diff of src/lib.rs" and press
  Enter. Focus moves to the diff heading (good). 2. Press Escape. 3. Tab to
  "Close diff" and press Enter.
- **Expected:** Escape closes the diff, and both Escape and Close diff
  return focus to the button that opened it, so she can go on to the next
  file.
- **Actual:** Escape does nothing (`ESC-IN-DIFF diffVisible=true`). Close
  diff moves focus to `#ovra-tabpanel`, a `tabindex=-1` div with no visible
  ring (`CLOSE-DIFF focus=div[tabpanel] "README.md" NO-RING`). Getting back
  to the next file's diff button takes Alt+Shift+G and nine Tabs.
- **Why it matters for Iryna:** reviewing a change set is "next file, read,
  stage", and each round trip costs a dozen key presses.
- **Suggested fix:** remember the opener (`data-action="diff"`, path,
  staged) in `showDiff` and focus it on close. Handle Escape in the diff
  section when focus is not in a text field.

### A11Y-46 minor: "Show git changes" and "Show git change at cursor" collide in the palette, and the wrong one takes the keyboard

- **Area:** ui/src/editor (commands.ts), ui/src/git (Git.svelte)
- **Steps:** 1. In the editor, on a changed line, open the palette and type
  "show git change". 2. Press Enter. 3. Type a few characters, expecting to
  be back in the editor.
- **Expected:** the command for the cursor line, or two names that differ
  early enough to type.
- **Actual:** the list is `["Show git changes  Alt+Shift+G", "Show git
  change at cursor"]`, so Enter opens the git panel and focuses the commit
  message. My next typing (" edited") went into the commit box instead of
  the code (`04f-forced-colors.png`).
- **Why it matters for Iryna:** she types palette commands from memory and
  does not see the list. A one-letter difference sent her text to the wrong
  place.
- **Suggested fix:** rename the editor command to start with its own word,
  for example "Git: change at cursor" / "Git: toggle blame", or the panel
  command to "Open git panel".

### A11Y-47 minor: in the popup for an unsaved file, focus starts on "Revert hunk"

- **Area:** ui/src/editor (git/extension.ts, popupTooltip mount)
- **Steps:** 1. Edit line 14 without saving. 2. Run "Show git change at
  cursor".
- **Expected:** focus on "Close" or on the status text "Save the file to
  stage this change", never on the destructive action.
- **Actual:** "Stage hunk" is disabled, so `mount` focuses the first enabled
  button, "Revert hunk" (`DIRTY focus=button "Revert hunk"`). Enter reverts
  the lines at once. Ctrl+Z brings them back (checked), but nothing says so.
- **Suggested fix:** focus "Close" when Stage is disabled, or always focus
  the dialog's first button but order the buttons Stage, Close, Revert. Say
  what kind of change it is in the label ("Changed line 14", "Deleted line
  after line 19"). Today it is always "Git change at line N", and for a
  deletion N is the next line.

### A11Y-48 minor: first Alt+Shift+G focuses an invisible heading

- **Area:** ui/src/git (Git.svelte)
- **Steps:** 1. Open `demo-app` fresh. 2. Press Alt+Shift+G before the status
  has loaded (within about 40 ms of the panel mounting, which is the normal
  case for the first press).
- **Expected:** focus on the commit message, as the command intends and as
  the second press does.
- **Actual:** the panel is still in "Loading git status…", so the command
  falls back to `#git-title`, a visually hidden `h2` (`OPEN-GIT ms=40
  focus=h2 "Git" NO-RING`, also at 512 px). A sighted keyboard user sees no
  focus at all. The screen reader says "Git, heading", which is fine.
- **Suggested fix:** wait for the model's first load (or a `tick` after
  `state.kind !== "loading"`) before choosing the focus target. Or focus the
  first focusable control once it appears.

### A11Y-49 minor: same names for different buttons, and icon buttons below 24 px

- **Area:** ui/src/git (FileSection.svelte, GitPanel.svelte)
- **Steps:** 1. Stage one hunk of `src/lib.rs`, so the file is in both lists.
  2. List the buttons (screen reader elements list). 3. Measure the row
  buttons. 4. Open "Recent commits".
- **Expected:** names that are unique and short, and targets of at least
  24x24 CSS px (WCAG 2.5.8).
- **Actual:** two buttons are both named "Show diff of src/lib.rs", one for
  the staged side and one for the unstaged side. Playwright's strict mode
  tripped on it twice. The ±, ↺ and + buttons are 22x17 with no gap
  between them. Commit items are named by their `title`, so each starts with
  the full 40-character hash: `listitem "21ef9db23ddc6c1b02bc63dc36c969f1bef38697
  Claude Say hello, world"`.
- **Why it matters for Iryna:** with RSI she misses 17 px targets. NVDA
  spells out 40 hex characters before every commit summary.
- **Suggested fix:** "Show staged diff of …" / "Show unstaged diff of …".
  Make the icon buttons at least 24x24 or add 2 px gaps. Drop `title` from
  the `li` (keep the 7-character id in the visible meta), or put the full
  hash in a visually hidden span after the summary.

### A11Y-50 polish: every hunk is a landmark

- **Area:** ui/src/git (GitDiff.svelte)
- **Steps:** open a diff with many hunks and list landmarks.
- **Actual:** each hunk is a `<section aria-label="Hunk 2 of 2, lines 24 to
  30">`, which is a region landmark, so a 40-hunk diff adds 40 landmarks next
  to banner, main and the sidebars.
- **Suggested fix:** use `role="group"` (or a heading per hunk, `h3 "Hunk 2
  of 2, lines 24 to 30"`, which gives the H key a way to jump between
  hunks).

## What worked well

- Every control in the panel has a clear name: "Stage src/main.rs",
  "Discard changes to src/main.rs…", "Mark README.md resolved (stage)",
  rows read "main.rs src, modified" and the status letter is
  `aria-hidden`, so git status is not colour-only.
- Focus after actions is handled with care: staging moves to the same action
  on the row now in place, discard asks in the row with focus on Cancel,
  Shift+Tab reaches Discard, Escape cancels, Undo puts focus back on the
  restored file, and staging a hunk keeps focus on the next "Stage hunk".
- Announcements through the panel's status region are clear and in plain
  words: "Staged src/main.rs.", "Discarded the changes to src/new_module.rs.
  Undo is available.", "Put back the changes…", "Committed cf21d48: …",
  "Created branch feature/a11y and switched to it.".
- A failing pre-commit hook shows its output in a `role="alert"` and keeps
  focus in the message box, so I could fix the message and retry.
- The branch menu is plain buttons with `aria-current` and `(current)`,
  Escape returns focus to the branch button, and a bad name shows a clear
  error next to the input.
- Ctrl+Enter commits from the message box and the textarea has
  `aria-keyshortcuts`; Amend changes the button to "Amend commit" and
  explains what it does.
- Hunk sections have useful labels ("Hunk 1 of 2, lines 1 to 5") and the
  added/removed lines keep their `+`/`-` signs, so the diff is not
  colour-only. All diff text is 10.9:1 or better.
- The panel reflows: no sideways scrolling at 640 or 512 px; long paths
  ellipsize and keep their full name for the screen reader. At 512x384 the
  panel is only 172 px tall inside the stacked layout but scrolls on its own.
- No animations or transitions in the panel, so `prefers-reduced-motion` has
  nothing to fight.
