# Keyboard power user (Oleh): round 10

Build: 7990e2b, headless, Playwright Chromium, 1280x800.
Scope: git panel redesign with pull and push, Get started last step, panel
hairlines and dock, Markdown transcript with Jump to latest and compact task
header, adding files to task boxes (drop, paste, `@`), file icons and git
colours, Focus toggle in the title bar, simplified project trust card.
Fixtures: `small`, `monorepo` (seed `small`; scripts add a bare `origin` with
one new commit for pull and push, and `.claude/settings.json` with a hook for
the trust card).

Headless runs in a browser, so the app's own remaps apply: Go to file is Alt+P,
Git is Alt+Shift+G, New task is Alt+N. All findings use those keys.

## Summary

Oleh can reach every feature in scope from the keyboard: the git list has good
row keys, the `@` menu adds files to a task without the mouse, Ctrl+Shift+.
toggles Focus on a Ukrainian layout too, and the trust card's buttons and
groups are all in Tab order. The repeated problem is focus after an action.
Commit, Get N commits, Send N commits, Trust project, Jump to latest, and the
Get started Open and Hide buttons all leave focus on `<body>`, so the next key
does nothing. The worst case is Get started's "Open" for a pending question:
the approval card appears but Y does nothing until Oleh tabs back to it. Other
gaps: pull and push have no palette commands, the Agents task field has an
almost invisible focus ring, and several global keys do nothing in Focus mode
without saying why.

## Findings

### KEY-1 major: Get started "Open" (and "Hide") drops focus to the page, so Y does not answer the question it opened

- **Area:** ui/src/agents (FirstSteps.svelte, AgentsPanel.svelte)
- **Steps:** 1. Alt+N, type "Fix the typo", Enter. 2. Go back to the task list
  (Tasks button). Get started now reads "3 Check Claude's work - Claude asks
  you something. Open". 3. Shift+Tab to "Open", press Enter. 4. Press Y.
- **Expected:** Open goes to the approval card (focus on the card or on "Allow
  once"), so Y or N answers right away, as Ctrl+Shift+A does.
- **Actual:** the task opens but `document.activeElement` is `<body>`. Y does
  nothing and the approval stays at "1 waiting" (s8 `03-open.png`,
  `04-after-y.png`). "Hide" on the checklist also leaves focus on `<body>`
  (s9 log: `after Hide steps: body#`).
- **Why it matters for Oleh:** this button exists to say "what to do now". When
  he does it, the next key is lost and he has to find the card with Tab.
- **Suggested fix:** have `ask` run the same code as `approvals.focus` (focus
  the first waiting card). `review` should focus the review's first item.
  After Hide, move focus to the next thing in the list (the trust card or the
  task field).

### KEY-2 major: Commit, Get commits and Send commits leave focus on `<body>`

- **Area:** ui/src/git (GitPanel.svelte, CommitBox.svelte, focus.ts)
- **Steps:** 1. Alt+Shift+G. Focus is on the first changed row, as it should
  be. 2. Shift+Tab to "Get 1 commit", Enter. 3. Tab to the commit message,
  type, Ctrl+Enter. 4. Shift+Tab to "Send 1 commit", Enter.
- **Expected:** after each action focus moves to the next sensible control:
  after pull, "Dismiss" or "What's new?" in the Got card. After a commit that
  leaves the tree clean, "Send 1 commit" (or the panel). After push, the
  panel's first control.
- **Actual:** all three end with `activeElement = body` (s4 log: `after pull
  Enter: body#`, `after Ctrl+Enter commit: body#`, `after push Enter: body#`).
  The pressed button disappears and nothing takes its place. The live region
  does say "Got 1 commit." and "Sent 1 commit.". The panel header comment
  promises "focus stays on the list after an action", but after a commit there
  is no list left.
- **Why it matters for Oleh:** commit then push is a two-key habit (Ctrl+Enter,
  then Enter on Send). Today he has to Alt+Shift+G and then Shift+Tab several
  times to reach Send.
- **Suggested fix:** in `GitModel.pull/push/commit`, after `tick()` focus in
  this order: the Got card's first button, `git-push`, the first row, then the
  panel heading (make it `tabindex=-1`). Add one test per action that checks
  `document.activeElement`.

### KEY-3 major: no palette command for pull or push

- **Area:** ui/src/git (Git.svelte commands)
- **Steps:** Ctrl+Shift+P, type "pull", then "push".
- **Expected:** "Git: get new commits" and "Git: send commits" (or pull and
  push in keywords). Like the other Git commands, they open the panel and run,
  or say "Nothing to send" when nothing is ahead.
- **Actual:** "pull" finds nothing. "push" finds only fuzzy noise: "Go to
  previous changed file in the tree" and "Git: go to previous change" (s2 log).
  The only way in is the panel button.
- **Why it matters for Oleh:** palette first is his habit. Git commands such
  as commit, new branch and switch branch are in the palette, but the two new
  actions are not. He concludes they don't exist.
- **Suggested fix:** register `git.pull` and `git.push` commands with titles
  that match the button words ("Git: get new commits", "Git: send commits")
  and the keywords `pull`, `push`, `sync`, `fetch`. Focus as in KEY-2.

### KEY-4 major: the Agents task and reply fields have a focus ring of 1.16:1

- **Area:** ui/src/agents (ContextInput.svelte / the composer box styles)
- **Steps:** Alt+N (or Tab to "Describe a task"). Look at the field.
- **Expected:** a clearly visible focus indicator, like the 2px teal outline
  the Focus mode task box and every button get.
- **Actual:** the textarea has `outline: none`. Its box gets `box-shadow: 0 0 0
  2px rgba(0,106,142,0.1)`, which is 1.16:1 against the white card and 1.06:1
  against the grey field (s6 `04-prompt-focused.png`; WCAG 1.4.11 asks for
  3:1). The reply field `#agent-reply` is the same. The Focus mode composer
  (`#focus-task`) shows a solid 2px ring, so the two task boxes disagree.
- **Why it matters for Oleh:** Alt+N, Ctrl+Shift+A and Escape all move focus
  in and out of this box. He cannot see whether his next keystroke will land
  there.
- **Suggested fix:** use the shared focus token (solid 2px `--focus`) on the
  composer box with `:focus-within`, as FocusComposer does.

### KEY-5 minor: Jump to latest drops focus to `<body>`, and there is no key for it

- **Area:** ui/src/agents (Transcript.svelte)
- **Steps:** 1. Run a task until the transcript scrolls. 2. Focus the
  transcript (it is `tabindex=0`) and press PageUp. "Jump to latest"
  appears. 3. Shift+Tab from the reply field to the button (it is the first
  stop) and press Enter.
- **Expected:** the view jumps to the end and focus goes back to the transcript
  or the reply field.
- **Actual:** the view jumps, the button unmounts, and focus is on `<body>`
  (s5 log: `after jump Enter: body# ... jump count=0`). End inside the focused
  transcript works natively, but nothing tells him that.
- **Why it matters for Oleh:** "jump to latest, then reply" is the normal
  loop, and it costs a Tab hunt every time.
- **Suggested fix:** in `jump()`, focus `#agent-reply` (or the log) before the
  button unmounts. Put "End: jump to latest" in the transcript's context keys
  (`registerContextKeys`, where "Agents") so Settings > Keyboard lists it.

### KEY-6 minor: Trust project leaves focus on `<body>`; Escape does not close the details

- **Area:** ui/src/agents/setup (ProjectTrust.svelte)
- **Steps:** 1. Open a project with `.claude/settings.json` that has a hook. 2. Tab to "Review and trust…", Enter. The groups and "Trust project" are in
  Tab order. 3. Press Escape. 4. Tab to "Trust project", Enter.
- **Expected:** Escape collapses the details and puts focus back on "Review
  and trust…". After trusting, focus moves to the card heading ("Project's own
  settings are on") or its Details button.
- **Actual:** Escape does nothing (`aria-expanded` stays true). After Trust,
  `activeElement = body` (s6 log: `after Trust Enter: body# heading=Project's
  own settings are on`). The arming delay itself works: an early Enter is
  ignored and the wait note is announced.
- **Why it matters for Oleh:** "Esc closes anything" is his model. Losing focus
  after a security decision also makes him unsure whether the press counted.
- **Suggested fix:** add an Escape handler on the details region that sets
  `details = false` and focuses the toggle. After `trust()` resolves, focus
  `#trust-heading` (already `tabindex=-1`, as used for the palette command).

### KEY-7 minor: in Focus mode, Alt+N, Alt+Shift+G and Ctrl+Shift+A do nothing and say nothing

- **Area:** ui/src/shell (Shell.svelte focus mode) with ui/src/agents and ui/src/git commands
- **Steps:** 1. Ctrl+Shift+. to enter Focus. 2. With focus on "Open IDE",
  press Alt+N, then Alt+Shift+G.
- **Expected:** Alt+N puts focus in "Give the agents a task". Alt+Shift+G
  either leaves Focus and shows Git, or says "Git is in the IDE (Ctrl+Shift+.)".
- **Actual:** both are silent no-ops (s7 log: focus stays on `focus-open-ide`,
  no palette). Ctrl+K, Ctrl+Shift+P and Alt+P do work in Focus mode. Escape
  does not leave Focus mode either.
- **Why it matters for Oleh:** he notices every key that silently does
  nothing. New task is the one key Focus mode is built around.
- **Suggested fix:** while Focus is on, point `agents.new_session` at
  `#focus-task`. For commands whose panel is hidden, leave Focus first (or
  announce why not). Consider Escape on an empty Focus task box returning to
  the IDE, in line with the Focus spec.

### KEY-8 minor: Tab does not accept an `@` suggestion; a half-typed mention is sent as plain text

- **Area:** ui/src/agents (ContextInput.svelte), ui/src/focus (FocusComposer.svelte)
- **Steps:** 1. In Focus mode, type "Explain @READ" in the task box. The menu
  offers README.md. 2. Press Escape (the menu closes), type "M", press Tab. 3. Back in the box, press Enter.
- **Expected:** Tab accepts the highlighted file, as in shells and VS Code.
  Typing after Escape reopens the menu. At the very least, an `@word` that
  matches no chip is flagged before Start.
- **Actual:** Tab moves focus to the mode select and adds no chip. Enter then
  starts a task named "Explain @READM" with no file attached (s9
  `02-after.png`). In the IDE box, Enter on the menu does add the chip, and
  the `@README` text is removed, so the task title reads "Fix the typo in"
  (s5 `02-started.png`).
- **Why it matters for Oleh:** with the drop and paste work this round, `@` is
  the keyboard way to attach files. Muscle memory (Tab) quietly sends the agent
  a broken reference.
- **Suggested fix:** handle Tab like Enter while the `@` menu is open, and
  reopen the menu when the caret is still in an `@query`. Keep the chosen
  file's name in the text (or in the task title) so the sentence still reads
  right.

### KEY-9 minor: no keyboard way to add tree-selected files to a task

- **Area:** ui/src/tree (menu.ts) with ui/src/agents (context API)
- **Steps:** 1. In the tree, select three files with Shift+Down. 2. Look for a
  way to send them to the task box: Shift+F10 row menu, then the palette.
- **Expected:** "Add to task" in the row menu (and a palette command) that does
  what dragging the selection onto the task box does.
- **Actual:** drag and drop (c26b952) is the only way to move a tree
  selection into the box. The row menu and palette have no such entry. `@`
  works only for one file at a time and needs the names retyped.
- **Why it matters for Oleh:** the new feature exists for mouse users only;
  he has already selected the files and should not have to type them again.
- **Suggested fix:** add a row-menu item "Add to task" (and a palette command
  for the current selection or the active editor file). It should call the
  same `attach` path as the drop, through the Agents context API, not by
  editing the tree's internals.

### KEY-10 polish: the Focus title bar button gives no state or shortcut to assistive tech

- **Area:** ui/src/shell (TitleBar.svelte)
- **Steps:** Tab to "Focus" in the title bar and inspect it.
- **Expected:** `aria-keyshortcuts="Control+Shift+Period"`, like the "Search or
  run" field next to it.
- **Actual:** `<button class="focus-switch tip" data-tip="Only the agents' work
  Ctrl+Shift+.">Focus</button>`. The shortcut appears only in the
  focus-visible tooltip. In Focus mode the "Focus" pill is plain text and the
  way out is a separate "Open IDE" button in the opposite corner.
- **Why it matters for Oleh:** a minor thing, but it is the one title bar
  control without `aria-keyshortcuts`, and the toggle has two different faces.
- **Suggested fix:** add `aria-keyshortcuts={keys[FOCUS]}`. Optionally make the
  Focus-mode pill the same button (pressed), so one control toggles both ways.

## What worked well

- Alt+Shift+G lands on the first changed row. Up/Down, Enter (diff), Escape
  (back to the same row), O and Delete work, and the row keys show under the
  list while it has focus. The More and branch menus open from Enter, and
  Escape returns focus to their buttons.
- Ctrl+Shift+. works from anywhere, puts focus on "Open IDE" on the way in and
  on the editor area on the way out. It also matched a dispatched Ukrainian
  layout event (a Ukrainian-layout `key`, `code "Period"`), and Alt+Shift+G matched with
  a Ukrainian `key` and `code "KeyG"`.
- The `@` menu in the IDE task box adds a chip on Enter, and the chip's
  "Remove" button is the next Tab stop.
- The palette shows "Focus: agents only Ctrl+Shift+." under "focus", and the
  shortcut appears in the title bar tooltip on keyboard focus.
- Splitter focus is clearly visible (teal bar), and arrows resize.
- Git colours in the tree keep text contrast: modified 5.90:1, untracked
  5.64:1 on the side panel, same as plain names (5.89:1). Status is also shown
  by a dot, not by colour alone.
- The trust card is short. Its buttons are in a sensible Tab order, and an
  early Enter cannot trust by accident.
