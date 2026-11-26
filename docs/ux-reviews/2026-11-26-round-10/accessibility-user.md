# Iryna (accessibility user): Round 10 (2026-11-26)

Build: 7990e2b on main, headless, Playwright Chromium 158.0.8127.36, 1280x800.
Also 1024, 800, 640 and 512 px wide (200% zoom of a 1280 px and a 1024 px
window). Dark and light colour schemes, `forced-colors: active` and
`prefers-reduced-motion: reduce`.
Scope: the Git panel redesign, the Get started checklist, panel hairlines and
the dock, Markdown in the transcript with Jump to latest and the compact task
header, files dropped or pasted into task boxes, file icons and coloured git
status, the Focus switch in the title bar, and the simplified trust card.
Fixtures: `small` (seeded) and `monorepo`. The scripted fake agent ran most
scripts. For Markdown, one script ran the real Claude driver against
`fake-claude` playing a one-turn fixture whose answer has headings, a list, a
table, code and 30 paragraphs. For the trust card, the script wrote a
`.claude/settings.json` with `Bash(*)`, a PostToolUse hook and
`ANTHROPIC_BASE_URL` into `small`. A MutationObserver logged what changed
inside `aria-live`, `role=status` and `role=alert`, as a stand-in for what a
screen reader says. Contrast was computed from computed styles, with parent
backgrounds and opacity blended in. Scripts s1 to s7 are in a scratch folder
and not committed.

## Summary

Iryna can do today's new jobs: read the Git panel and commit, add context to a
task without dragging (the @ button and the menu are a proper combobox), read
the trust card's warnings and trust it, and use Focus. She can do all of it by
keyboard and screen reader. The worst problems:

- The transcript's live region reads the agent's raw Markdown aloud.
- Jump to latest drops focus on the page body.
- At 200% zoom, the Focus button sits on top of the search field.
- In the Git panel, a file's status is shown only by a 6 px coloured dot, and
  the dot disappears in forced colours.
- Trust project still sends focus to the file tree.

Overall the new surfaces start from a good base: names, lists, `aria-current`
on the checklist, status colours that pass 4.5:1 in both themes, and reduced
motion. The problems are in focus handling and in what is shown only visually.

## Findings

### A11Y-1 major: the transcript's live region reads the agent's raw Markdown

- **Area:** ui/src/agents (`Transcript.svelte`, `spoken`)
- **Steps:** 1. Open `demo-app` and start the task "Summarize the project."
  2. The agent answers in Markdown (`# Summary`, `**small**`, a list, a table,
  a code fence).
- **Expected:** The polite region says the answer as text: "Claude: Summary.
  This is a small Rust app. Files: src/main.rs prints hello …", the same text
  the rendered message shows.
- **Actual:** The polite region (`p.spoken[aria-live=polite]`) gets
  `message.text` before rendering: "Claude: # Summary This is a **small** Rust
  app. ## Files - `src/main.rs` prints hello … | Step | Who | |---|---| | Fix
  typo | agent | `rust fn main() {} ` > Note: …". NVDA reads "number sign
  Summary … star star small star star … vertical line Step vertical line …".
  The visible message is rendered correctly (`transcript.png`).
- **Why it matters for Iryna:** This line is how she follows the agent without
  moving focus. Since 321c8b0, most answers contain Markdown, so most
  announcements are now noise. It also uses up the `MAX_SPOKEN` budget on
  punctuation.
- **Suggested fix:** Build `spoken` from the rendered text: run `renderChat`,
  take `textContent` and join blocks with ". ". Or strip Markdown with
  micromark's plain-text output. Drop code fences and tables, or summarise
  them as "(code)" and "(table)". Add a test with `**bold**` and a table.

### A11Y-2 major: Jump to latest drops focus on the page body

- **Area:** ui/src/agents (`Transcript.svelte`, `jump()`)
- **Steps:** 1. Open a finished task with a long transcript. 2. Scroll the log
  up: the round "Jump to latest" button appears under it (`jump.png`). 3. Tab to
  it and press Enter.
- **Expected:** The log scrolls to the end and focus moves somewhere useful:
  the last message, or the Reply field under the log.
- **Actual:** The log scrolls. Then `detached` turns false and the button is
  removed while it has focus, so `document.activeElement` is `BODY` (logged
  "after jump, activeElement: BODY"). The next Tab starts from the top of the
  page.
- **Why it matters for Iryna:** With a keyboard and a screen reader, losing
  focus means starting over from the title bar, which is about 30 Tab presses
  back to the Reply box. It happens every time she uses the button that was
  made for her situation.
- **Suggested fix:** In `jump()`, move focus before the button goes, to the
  Reply field (`#agent-reply` or similar), or to the last message (give it
  `tabindex="-1"`). Optionally announce "Showing latest". Add a component test
  that checks `document.activeElement` after the click.

### A11Y-3 major: at 200% zoom the Focus button covers the search field

- **Area:** ui/src/shell (`TitleBar.svelte`)
- **Steps:** 1. Open `demo-app`. 2. Make the window 640 px wide (200% zoom of
  1280 px) or 512 px wide (200% of 1024 px).
- **Expected:** The title bar's three groups fit or collapse, and nothing
  overlaps.
- **Actual:** The `.trail` group with the new Focus button overflows onto the
  middle column. At 640 px, Focus spans x 476 to 532 and the search field
  spans x 142 to 502, so they overlap by 26 px, on top of its "Ctrl+K" kbd
  (447 to 496). At 512 px they overlap by 90 px: "Focus" is drawn over
  "Ctrl+K" and touches the Reviews icon (`zoom-640.png`, `zoom-512.png`). The
  project switcher shrinks to 41 px, then to 16 px, and shows one letter. At
  1024 and 800 px it fits.
- **Why it matters for Iryna:** She works at 200%. The two labels are drawn
  over each other, and the place she clicks for search hits Focus instead
  (or the other way round).
- **Suggested fix:** Below about 700 px, make Focus an icon button with an
  `aria-label` (as Sidebar and Agents are), or move it into More. Or let the
  search field shrink to its icon. Give `.trail` `flex: none` and
  `min-width: max-content` so the grid gives it room instead of overlapping.
  Add a layout test at 640 and 512 px that checks the bounding boxes do not
  intersect.

### A11Y-4 major: Git panel status is a colour-only 6 px dot that disappears in forced colours

- **Area:** ui/src/git (`FileSection.svelte`); ui/src/tree for the folder dot
- **Steps:** 1. Open `demo-app` and the Git panel (two changes: `main.rs`
  modified, `new_module.rs` untracked). 2. Look at the rows in light and dark.
  3. Turn on forced colours (Windows High Contrast) and look again.
- **Expected:** Each row shows its status in a way that does not rely on
  colour, as the file tree does since ffe1266 (a coloured name plus "M" or
  "U"). The status stays visible in forced colours.
- **Actual:** The only visible status is `span.dot` (6x6 px, `background`
  `--warn` or `--ok`, empty text). Modified and untracked differ only in hue
  (`git-light.png`). The letter is in a `title` (hover only). Screen readers do
  get ", modified" through the visually hidden text, which is good. In forced
  colours the dot's background becomes Canvas (`rgb(0,0,0)` on black), so the
  status is gone and both rows look identical (`forced-git.png`). The file
  tree's folder dot (`.dot.git-modified` on `src`) disappears the same way.
- **Why it matters for Iryna:** At 200% zoom with low vision she reads by
  shape and text. A 6 px dot in one of two similar tones is not enough, and in
  high-contrast mode there is nothing at all. The tree and the Git panel now
  say the same thing in two different ways.
- **Suggested fix:** Use the tree's treatment in `FileSection`: the status
  letter (`CHANGE_LETTER`) as text in the status colour, `aria-hidden`, next
  to the existing hidden word. For dots that remain (tree folders), add
  `@media (forced-colors: active) { .dot { background: CanvasText; } }` or
  `forced-color-adjust: none`.

### A11Y-5 major: Trust project still sends focus to the file tree (A11Y-27 from round 7, still open)

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte`)
- **Steps:** 1. Open `demo-app` with a `.claude/settings.json` (broad rule,
  hook, `ANTHROPIC_BASE_URL`). 2. In the trust card, press Enter on "Review and
  trust…" (focus stays on it, `aria-expanded=true`, good). 3. Tab to "Trust
  project" and press Enter.
- **Expected:** Focus stays in the card, on the new heading "Project's own
  settings are on" (it already has `tabindex="-1"`) or on "Details", so she
  hears the result.
- **Actual:** Focus goes to `DIV "Files"`, the file tree ("after trust,
  active: DIV# Files"). Nothing is announced. The card turns into one line,
  "Project's own settings are on", with the button "3 warnings Details"
  (`trust-after.png`).
- **Why it matters for Iryna:** She has just made a security decision and
  cannot hear that it worked. To check, she has to find the Agents region
  again.
- **Suggested fix:** After `trust(true)` resolves, focus `#trust-heading`
  (`tick()` and then `focus()`). Or focus the Details toggle. Do not let the
  project refresh move focus to the tree. Add a test on `activeElement`.

### A11Y-6 minor: the agent's Markdown headings join the page outline as h1 to h3

- **Area:** ui/src/agents (`markdown.ts`, `AgentText.svelte`)
- **Steps:** 1. Run a task whose answer starts with `# Summary` and has `##`
  and `###`. 2. List the page headings (NVDA H, or Insert+F7).
- **Expected:** Headings inside a message sit under the task heading (h3),
  so the outline still says where she is.
- **Actual:** The outline is: H1 Ovra, H2 File tree, H2 demo-app, H2 Agents,
  H3 "Summarize the project.", then **H1 Summary, H2 Files, H3 Next steps**.
  Pressing 1 in NVDA jumps from the app's name to the agent's message. Every
  long answer adds headings at the app's top levels.
- **Why it matters for Iryna:** She moves between panels by heading levels.
  Agent text that claims level 1 and 2 breaks that map.
- **Suggested fix:** In `renderChat`, shift heading levels to start below the
  message's context (h4 to h6, clamped). Or turn them into
  `<p role="heading" aria-level=…>` with a computed level. Keep their look
  through CSS classes. Add a test in `markdown.test.ts`.

### A11Y-7 minor: panel hairlines are 1.25:1 and vanish in forced colours under the title bar

- **Area:** ui/src/shell (`theme.css` `--separator`, `Shell.svelte`,
  `Splitter.svelte`)
- **Steps:** 1. Open `demo-app` in dark, then light. 2. Measure the hairlines
  that d544fc3 added between Files, main, Agents, the title bar, the status
  bar and the dock. 3. Repeat with `prefers-contrast: more` and forced colours.
- **Expected:** Where a hairline is the only visible edge of a draggable
  splitter or of a panel, it reaches 3:1 (WCAG 1.4.11), at least with
  increased contrast on.
- **Actual:** `--separator` blended over its surface measures 1.24:1 on
  `--s1` and 1.26:1 on `--s2` in dark (8% white), and 1.25:1 in light (10%
  black). With increased contrast it is still 2.18:1 to 2.23:1. The panels'
  tones differ by about 1.04:1 in dark, so the hairline is the only edge, and
  it is the only resting sign of the splitters. In forced colours the
  splitters get `CanvasText` borders (good). The title bar's hairline is a
  `box-shadow`, which forced colours remove, so the title bar and the panels
  run together (`forced-tree.png`).
- **Why it matters for Iryna:** With low vision she finds the panel edges and
  the resize handles by these lines. At 1.25:1 she cannot see them.
- **Suggested fix:** Under `prefers-contrast: more`, raise `--separator` to
  at least 3:1 (about 40% white in dark and 45% black in light). Draw the
  title bar line with `border-bottom` instead of `box-shadow`, so forced
  colours keep it. The default hairline can stay quiet, because it is
  decorative where the tone already differs.

### A11Y-8 minor: the Focus switch has no state, and Focus mode has two texts below contrast

- **Area:** ui/src/shell (`TitleBar.svelte`); ui/src/focus (`FocusBar.svelte`,
  `FocusView.svelte`)
- **Steps:** 1. Open `demo-app` and inspect the title bar. 2. Press Enter on
  Focus. 3. Measure the text in Focus mode (light).
- **Expected:** The switch for a mode says it is a switch, as the Sidebar and
  Agents toggles beside it do (`aria-pressed`). Text in Focus mode keeps
  4.5:1.
- **Actual:** `button[data-testid=focus-switch]` has no `aria-pressed` and no
  `aria-keyshortcuts`. Its name is "Focus", plus whatever Chromium takes from
  the CSS tooltip ("Only the agents' work Ctrl+Shift+."), so other browsers
  may say only "Focus". That could also be read as "move focus". On press,
  focus goes to "Open IDE", which is good, and the banner then says "demo-app
  Focus" as plain text. The "Focus" pill (`--accent` on `--accent-soft`, 12
  px) measures 4.28:1. "Nothing waits for you." (`p.quiet`, `--text-3`, 14 px)
  measures 3.47:1, and the count "1" next to "Done" measures 3.47:1
  (`focus-on.png`).
- **Why it matters for Iryna:** She hears "Focus, button" with no idea what
  it changes or whether it is on. In Focus mode, the line that says nothing
  waits is the main status of the screen, and it is too faint.
- **Suggested fix:** Give the switch `aria-label="Focus mode"`,
  `aria-pressed={focusOn}` (false in the IDE) and `aria-keyshortcuts`. Or move
  the tooltip text into `aria-describedby`. In Focus mode use `--text-2` for
  `p.quiet` and `.n`, and `--text` on the pill, or a stronger soft fill.

### A11Y-9 minor: a chip added from the @ menu is not announced, while a pasted one is

- **Area:** ui/src/agents (`ContextInput.svelte`, `choose()`)
- **Steps:** 1. In "Describe a task", press Enter on "Add context" (the field
  gets "@" and `aria-expanded=true`, good). 2. Type "lib" and press Enter on
  `lib.rs`. 3. Paste a file `notes.txt` into the field. 4. Remove `lib.rs`
  with its "Remove lib.rs" button.
- **Expected:** Each change to the context list is said once: "Added lib.rs",
  "Added notes.txt", "Removed lib.rs".
- **Actual:** The live region `.ctx-said` said "Added notes.txt" and "Removed
  lib.rs", but nothing for `lib.rs`. `choose()` adds the chip without setting
  `said`. The chip list itself (`list "Context sent with the message"`) is
  well named.
- **Why it matters for Iryna:** With the menu closed and the caret back in the
  field, she cannot tell whether her pick became a chip or failed.
- **Suggested fix:** In `choose()`, after `drafts.add` resolves, set
  `said = \`Added ${option.label}\``. Add the case to the existing
  ContextInput tests.

### A11Y-10 minor: the Focus task box has no non-drag button to add files

- **Area:** ui/src/focus (`FocusComposer.svelte`)
- **Steps:** 1. Turn on Focus. 2. Look for a way to add a file to "Next task"
  without dragging.
- **Expected:** The same "Add context" (@) button as the panel's box, since
  7841dbb gives Focus the same chips.
- **Actual:** The composer has the textbox "Next task", the "How" combobox and
  Start. Typing "@" does open the menu, but nothing on screen or in the field's
  description says so. The only visible way in is drag and drop.
- **Why it matters for Iryna:** She cannot drag with RSI or a screen reader,
  and she will not guess "@".
- **Suggested fix:** Render the panel's "Add context" button in
  `FocusComposer` (it calls `context.open()` or inserts "@"). Or add "@ adds
  files" to the field's `aria-describedby` hint.

### A11Y-11 minor: Get started says "Claude is working" after the task failed

- **Area:** ui/src/agents (`FirstSteps.svelte`, `AgentsPanel.svelte`)
- **Steps:** 1. With a `claude` that fails to start, type a task and press
  Start. 2. Read the checklist.
- **Expected:** Step 3 says something true, for example "The task failed. Open
  it to see why", or it stays on step 2.
- **Actual:** The task list says "Failed: claude exited…" and an alert says
  "Claude could not start…". The checklist still reads "Type a task and press
  Start, done; Check Claude's work: Claude is working. Its questions and
  changes show up here." `isStarted` only checks that a task exists.
- **Why it matters for Iryna:** She uses the checklist as the summary of where
  she is. A screen reader user hears a reassuring line that contradicts the
  alert just before it.
- **Suggested fix:** Pass the newest task's state to `FirstSteps`. On failure,
  show a detail like "The task stopped. Open it to see why." with an Open
  button. Add a case to `FirstSteps.svelte.test.ts`.

### A11Y-12 polish: the compact task header's "Close" button has no object

- **Area:** ui/src/agents (`AgentsPanel.svelte`)
- **Steps:** 1. Open a task. 2. Tab through the header.
- **Expected:** Button names say what they act on, as the list's "Close
  Summarize the project." does.
- **Actual:** In the header she hears "Tasks, button", "Close, button", then
  the heading. "Close" could close the panel, the task or the view. The line
  "Sandboxed · gaps" uses a word she cannot act on without opening Details.
- **Suggested fix:** Use `aria-label="Close task <name>"` (or "Back to tasks"
  for the other button if that is what it does). Write the sandbox line as
  "Sandboxed, with gaps: see Details".

## What worked well

- The file tree's new status keeps both the letter (M, U) and the word in the
  row's description ("main.rs modified", "src contains modified files"). The
  status colours pass in both themes: 9.2, 8.7, 6.5 and 8.5:1 in dark, and
  5.9, 5.6, 5.8 and 5.4:1 in light, and 4.7:1 or more on the focused
  selection.
- File icons in the tree, tabs, palette and Git panel are `<img alt="">` or
  `aria-hidden`, so names are not doubled.
- The Git panel is a good keyboard citizen. It has one Tab stop per list
  (roving `tabindex`), Arrow keys move between rows, Shift+F10 opens the row
  menu on "Show diff", and the row keys ("Enter: show the diff. Delete:
  discard. O: open the file.") are linked through `aria-describedby` while the
  list has focus. Row buttons read "main.rs src, modified".
- The trust card marks every warning with "⚠" and `role="note"`, has no
  contrast failures in either theme, and "Review and trust…" keeps focus and
  sets `aria-expanded`.
- The checklist is an `<ol>` with ", done" in hidden text and
  `aria-current="step"` on the next step, and its heading carries the count
  ("Get started 1 of 3").
- Adding context without dragging works by keyboard: "Add context" opens a
  real combobox (`aria-expanded`, `aria-activedescendant`, `listbox "Add
  context"`), chips are a named list with "Remove <file>" buttons, and focus
  returns to the field after a removal. A pasted file is announced.
- Rendered Markdown keeps its semantics: lists, `table` with `columnheader`,
  `blockquote`, `code`, and links with their target in `title`.
- Reduced motion is respected through the duration tokens, and the
  live-activity pulse stops. Nothing scrolls sideways at 640 or 512 px.
