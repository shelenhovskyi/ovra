# Keyboard power user and accessibility user: git agent actions

Build: 8a385a3, headless, Chromium 158.0.8127.36, 1280x800 (and 1024x700 at
200% zoom, plus `forced-colors: active`).
Scope: the Agent actions button and menu in the Git sidebar view, the result
line and cards, the drafted message with Undo, and the "Git: ..." palette
commands (`docs/specs/git-agent-actions.md`). Fixtures: `small`, seeded. Before
the page loaded, a `feature` branch was merged with `--no-ff` so `ORIG_HEAD`
exists, then `README.md` was changed and `new.txt` added. Scripted fake agent.

Findings are tagged **[KEY]** (Oleh, keyboard only) or **[A11Y]** (Iryna,
screen reader, 200% zoom, forced colours). Some apply to both.

## Summary

Opening the menu, moving through it and choosing an action work well from the
keyboard, and the result card takes focus on its heading. But a suggested
commit message cannot be edited: every keystroke in it sends focus back to
the card heading, so only the first character lands, with keyboard or mouse.
Focus is also lost to `<body>` while the agent works and after Dismiss, the
menu goes off-screen at 200% zoom, and in forced colours the focused menu item
can't be seen. Contrast is good everywhere.

## Findings

### KEY-1 blocker [KEY][A11Y]: typing in a suggested commit message moves focus to the card heading after one character

- **Area:** ui/src/git (`AssistCard.svelte`)
- **Steps:** 1. Alt+Shift+G, Shift+Tab to Agent actions, Enter, Enter
  (Suggest commits). 2. The card appears with focus on "2 suggested
  commits". 3. Tab into "Message of commit 1", End, type `XY`. 4. Click the
  same textarea and type `Z`.
- **Expected:** the message reads `docs: greet the worldXY`, then `...XYZ`,
  and focus stays in the textarea.
- **Actual:** after the first character, focus jumps to the `h3`
  (`FOCUS after typing XY: H3 ... 2 suggested commits`) and the value is
  `docs: greet the worldX`. The `Y` goes nowhere. Clicking and typing gives
  the same result (`...worldXZ`, focus on the H3 again). Typing a body
  (Enter, `Body line.`) is lost too: the commit was made as
  `docs: greet the world` with no body. "Leave out" also sends focus to the
  heading, through the same path.
- **Why it matters:** editing the message in place is half of the card's
  purpose in the spec. Oleh can't change a message without typing one
  character at a time and tabbing back. A screen reader user hears the
  heading again after every keystroke.
- **Suggested fix:** the `$effect` that focuses `[data-first]` depends on
  `current` (`assist.state`), and `assist.edit()` and `assist.drop()`
  replace that state, so the effect runs again on every change. Focus only
  when a new result arrives, for example by keying on the state's `kind` or
  on a result id that is untracked or kept in a plain variable. Add a
  component test that types two characters into a plan message and checks
  both the value and `document.activeElement`.

### KEY-2 major [KEY][A11Y]: focus drops to `<body>` after choosing an action and after Dismiss

- **Area:** ui/src/git (`AgentActions.svelte`, `AssistCard.svelte`)
- **Steps:** 1. Open the menu, Enter on "Suggest commits". 2. Check
  `document.activeElement` right away. 3. Later, with the "What's new" card
  shown, Tab to Dismiss and press Enter. 4. Do the same on the plan card.
- **Expected:** while the agent works, focus is on the result line (its Stop
  button) or stays on the Agent actions button. After Dismiss, focus returns
  to the Agent actions button, as it already does after Commit all.
- **Actual:** `FOCUS right after choose: BODY` (`choose()` closes the menu
  while its focused item is removed). `FOCUS after Dismiss: BODY` for both
  cards. The next Tab starts again from the top of the page (the top bar),
  about 10 to 15 stops away from the git panel.
- **Why it matters:** Oleh can't press Stop without hunting for it, and Esc
  has nothing to act on. With a real agent this "lost" state lasts as long as
  the run. Iryna's screen reader says nothing or reads the document title,
  with no hint of where she is.
- **Suggested fix:** in `choose()`, move focus before closing the menu: to the
  Stop button of the running line, or back to `toggle`. In every Dismiss
  handler (plan, text, error), focus `[data-testid=git-assist]` after the
  card is removed, the same way `commitAll()` does. Escape while the result
  line has focus could map to Stop.

### A11Y-1 major [A11Y]: at 200% zoom in a 1024 px window, the menu opens off-screen to the left

- **Area:** ui/src/git (`AgentActions.svelte`)
- **Steps:** 1. Set the window to 1024x700 at 200% zoom. 2. Focus Agent
  actions and press Enter.
- **Expected:** the whole menu is visible, either shifted to fit the viewport
  or wrapping its text.
- **Actual:** `menu rect zoom {"x":-192, ... "width":400}`. Half the menu is
  outside the window, and the item labels show only their last letters ("e",
  "w") (screenshot `11-zoom-menu`). `right: 0`, `min-width: 200px` and
  `white-space: nowrap` push it past the left edge of the narrow sidebar.
- **Why it matters:** at her normal zoom Iryna can't read which item she is
  on. She has to rely on the screen reader alone, or zoom out.
- **Suggested fix:** keep the menu inside the viewport. Measure on open and
  flip to `left: 0` when `right: 0` would clip, or clamp it with
  `max-width: calc(100vw - 16px)` and allow wrapping. The branch menu has the
  same layout risk, so share one approach with it.

### A11Y-2 major [A11Y]: forced colours show no focused menu item, and the menu has no boundary

- **Area:** ui/src/git (`AgentActions.svelte`)
- **Steps:** 1. Emulate `forced-colors: active`. 2. Open the menu.
- **Expected:** the focused item has a visible outline or system Highlight,
  and the menu has a visible edge.
- **Actual:** every item gets the same system button border. The focused one
  looks like the others, because `.menu button:focus-visible` sets
  `outline: none` and relies on `background: var(--accent)`, which forced
  colours remove. The menu has no border and sits on top of the "Commit
  message" label (screenshot `12-forced-colors-menu`).
- **Why it matters:** high-contrast users can't tell which action Enter will
  run.
- **Suggested fix:** use `outline: 2px solid transparent` instead of
  `outline: none` (it shows in forced colours), or add
  `@media (forced-colors: active) { .menu button:focus-visible { outline: 2px solid Highlight; } .menu { border: 1px solid CanvasText; } }`.

### A11Y-3 minor [A11Y]: the "working" status and the "replaced your message" hint may not be announced

- **Area:** ui/src/git (`AssistCard.svelte`, `CommitBox.svelte`)
- **Steps:** 1. Choose any action and listen. 2. Type your own message, then
  run "Git: write commit message".
- **Expected:** "Suggesting commits…" is announced when it starts. After a
  draft, the reader says that the agent replaced her text and that Undo is
  available.
- **Actual:** the `<p role="status">` is created at the same moment as its
  text. NVDA and VoiceOver often skip a live region that appears with its
  content already in it. (The fake agent answers in about 110 ms, so this
  comes from reading the code; it could not be heard in the run.) After a
  draft, focus goes to the commit textarea, which reads
  `chore: update the staged files`. "An agent replaced your message. Undo"
  is a plain paragraph, not linked to the box and not live.
- **Why it matters:** Iryna may not know her own text was replaced, or that
  Undo exists.
- **Suggested fix:** keep an always-present `role="status"` container in the
  panel and put the running text, "Message drafted by an agent", and the
  result headings into it. Give the textarea
  `aria-describedby` pointing at the hint while it is shown.

### KEY-3 minor [KEY]: Esc does not dismiss a result card, and Ctrl+Enter does nothing in a card message

- **Area:** ui/src/git (`AssistCard.svelte`)
- **Steps:** 1. With the plan card focused on its heading, press Escape. 2. In "Message of commit 1", press Ctrl+Enter.
- **Expected:** Escape dismisses the card (or stops a running action), as
  "Esc closes anything" elsewhere. Ctrl+Enter acts like Commit all, the same
  way it commits from the main message box.
- **Actual:** `plan after esc: 1` and `plan after ctrl+enter: 1`. Nothing
  happens either time.
- **Why it matters:** these are extra Tab presses on each use, and the
  shortcut works in one message box but not the other.
- **Suggested fix:** handle Escape on the card (dismiss, or stop while
  running, then focus the toggle). Handle Ctrl/Cmd+Enter in the plan
  textareas as Commit all, and add `aria-keyshortcuts`.

### KEY-4 minor [KEY][A11Y]: the menu only partly follows the menu-button pattern

- **Area:** ui/src/git (`AgentActions.svelte`)
- **Steps:** 1. Focus Agent actions and press ArrowDown. 2. Open the menu and
  press Tab, then Shift+Tab twice. 3. Press `w`.
- **Expected:** ArrowDown (and Up) on the button opens the menu. Each item
  has `tabindex=-1`, so Tab leaves and closes the menu. A letter jumps to the
  matching item.
- **Actual:** ArrowDown on the button does nothing. Tab moves from item to
  item and the menu stays open (`menu open after tab: 1`). The first
  Shift+Tab returns to the button with the menu still open, and only the
  second one closes it. Typeahead does nothing. Up, Down, Home, End,
  Escape, Enter and Space all work correctly.
- **Why it matters:** screen reader users expect APG behaviour in a
  `role=menu`, and Oleh gets extra Tab stops.
- **Suggested fix:** use a roving `tabindex` (items `-1`). On Tab, close the
  menu and let focus move on. Open on ArrowDown/ArrowUp from the button and
  focus the first or last item. Add first-letter typeahead.

### KEY-5 minor [KEY]: "Summarize what's new" is listed whenever there is a HEAD, not only after a pull, merge or rebase

- **Area:** ui/src/git (`assist.svelte.ts`, `Assist.available`)
- **Steps:** 1. In a repository with no `ORIG_HEAD`, open the menu.
- **Expected:** the item appears only when `ORIG_HEAD` exists (spec table:
  "Shown when HEAD moved by a pull, merge or rebase").
- **Actual:** `if (status.head) actions.push("pulled")` adds it in every
  repository with a commit. Choosing it gives the error line "There are no
  new commits since the last pull, merge or rebase." (from reading the
  code). The merged fixture in this run had `ORIG_HEAD`, so the item was
  correct there.
- **Why it matters:** this is an item that always fails in a menu meant to
  show only what applies, and it costs keystrokes plus an error to dismiss.
- **Suggested fix:** add an `orig_head` (or similar) flag to the status the
  UI already gets, or call `git.pulled` when the menu opens, and filter on
  it.

### KEY-6 polish [KEY]: Ctrl+Z after a drafted message doesn't bring the old text back

- **Area:** ui/src/git (`CommitBox.svelte`)
- **Steps:** 1. Type `my own words` in the commit box. 2. Run "Git: write
  commit message". 3. Press Ctrl+Z.
- **Expected:** the old text comes back, as it would after any replace.
- **Actual:** `box after ctrl+z: "chore: update the staged files"`. The value
  is set from code, so the browser's undo history doesn't include it. The
  Undo button one Tab away does restore `my own words`, and focus returns to
  the box.
- **Suggested fix:** set the draft with
  `document.execCommand("insertText")` after selecting all (this keeps native
  undo), or handle Ctrl+Z on the box while `replaced` is set.

### A11Y-4 polish [A11Y]: the menu items' explanations are only in hover tooltips

- **Area:** ui/src/git (`AgentActions.svelte`)
- **Steps:** Hover over, or focus, "Suggest commits".
- **Expected:** a keyboard user can see what the item does, for example "You
  commit them".
- **Actual:** the hint is only in `title`, which shows on mouse hover. Keyboard
  focus shows nothing, and the text isn't visible at 200% zoom.
- **Suggested fix:** keep `title` for screen readers if you like, and add a
  visible one-line description under the focused item, or a short footer
  line in the menu ("The agent only proposes; you commit.").

## Not in scope, noted in passing

- After each successful action, the Agents column lists the session as
  "Stopped" (`Suggest commits for 4 chang… Stopped`), which reads like a
  failure. "Get started" also ticks "Type a task and press Start" because of a
  git action. Both belong to the Agents column (orchestrator round).
- The task box's footer changed to "Works in this folder, without a sandbox"
  after the research sessions ran. That is for the security reviewer to
  check.
- The fake agent answers in about 110 ms, so the "working" line and its Stop
  button could not be tested from the keyboard. KEY-2 and A11Y-3 cover them
  from the code.

## What worked well

- The toggle has an accessible name ("Agent actions"), `aria-haspopup="menu"`
  and `aria-expanded`, and a 28x28 target. Its icon contrast is 5.89:1 on
  the sidebar, and its focus ring is clear.
- Enter and Space open the menu with focus on the first item. Up and Down
  wrap, Home and End work, and Escape closes the menu and returns focus to
  the button. The menu lists only what applies (Suggest commits, Write commit
  message, Summarize what's new).
- A new result card takes focus on its heading ("2 suggested commits", "What's
  new"). The card is a named region, each message textarea is labelled
  "Message of commit N", and the Leave out buttons are named per commit.
- After Commit all, focus returns to Agent actions, and "Committed 256bf6f:
  docs: greet the world" is announced through the commit status.
- "Write commit message" puts focus in the message box with the draft. The
  Undo button is the next Tab stop and restores the person's text.
- All the commands are in the palette ("Git: suggest commits", ...). Running
  one that doesn't apply says "Explain conflicts does not apply now." and
  puts focus in the panel.
- Text contrast in the cards is 6.68:1 (quiet buttons, file lines, hints), and
  Commit all is 6.08:1.
