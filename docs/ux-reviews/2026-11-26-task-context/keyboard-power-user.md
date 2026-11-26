# Oleh, the keyboard power user: task context (@)

Build: 43cf23e, headless, Chromium 158.0.8127.36, 1280x800.
Scope: adding context to agent tasks with `@` (`docs/specs/task-context.md`):
the @ menu in "Give Claude a task" and "Reply to Claude", chips, Include on
amber chips, Backspace on an empty box, Ask agent from the editor menu
(Shift+F10), the problem hover card and the terminal menu.
Fixtures: `small` (plus a `.env` file for the amber chip, and a `// TODO`
line so `fake-lsp` reports a problem once the project is trusted),
`monorepo` (15,250 files) for speed. Scripted fake agent.

## Summary

Oleh can do the whole flow without a mouse: Alt+N to the task box, `@`,
type, arrows, Enter or Tab, then Enter to start. The menu is fast even in
the monorepo, and the arrow keys, Escape and Backspace use `event.code`.
The worst problems: focus drops to `<body>` after Include or a chip's ×;
holding Backspace to clear the text also deletes every chip without a word;
and the natural keyboard path to a problem (F8, then Shift+F10, Ask agent)
attaches a selection instead of the problem. Overall it feels fast, but a
few traps around focus and chips cost him context he picked on purpose.

## Findings

### KEY-1 major: focus is lost to `<body>` after Include or after removing a chip with ×

- **Area:** ui/src/agents (`ContextInput.svelte`)
- **Steps:** 1. Alt+N, type `Check @.env`, Enter: an amber `.env` chip with
  "May hold secrets" and Include. 2. Tab to Include, Enter. 3. Separately:
  add `@lib` and `@main` as chips, Tab to "Remove lib.rs", Enter.
- **Expected:** focus stays in the box: on the chip's × after Include, and
  on the next chip's × (or the text field when none is left) after a
  removal.
- **Actual:** `FOCUS after Include: BODY` and `FOCUS after x Enter: BODY`
  (session log). Both buttons are removed from the DOM by their own
  action, so focus falls to the document. The next Tab starts from the top
  of the page.
- **Why it matters for Oleh:** Include is the step Ovra requires before it
  sends an amber chip. After every Include or removal he has to find his
  way back to the task box (Alt+N works, but he has to know that focus is
  gone; nothing on screen shows it).
- **Suggested fix:** after Include, focus the chip's × button (same `li`);
  after a removal, focus the next chip's ×, else the previous one, else the
  field (`field.focus()`). Add a component test that checks
  `document.activeElement` after both actions.

### KEY-2 major: holding Backspace to clear the text also deletes every chip, without notice or undo

- **Area:** ui/src/agents (`ContextInput.svelte`, `keydown`)
- **Steps:** 1. Add two chips (`lib.rs:1-2`, `strings.rs`) and type some
  text (27 characters). 2. Hold Backspace to clear the text (simulated as 32
  presses: key repeat sends the same keydown events).
- **Expected:** the text is cleared and the chips stay, or at least the
  chips go only on a fresh Backspace press, one at a time, with a visible or
  spoken note.
- **Actual:** text and both chips are gone (`after 32 backspaces (text 27
  chars, chips 2): chips []`). Nothing says chips were removed, and there is
  no way to get them back except picking them again. The selection chip in
  particular cannot be re-added once the editor selection changed.
- **Why it matters for Oleh:** he clears fields by holding Backspace or with
  Ctrl+Backspace bursts, as in every terminal. He would send the task
  without the context he chose and only find out from the agent's answer.
- **Suggested fix:** ignore `event.repeat` for the chip-removing Backspace
  (only a fresh press removes a chip), and announce the removal in a polite
  live region ("Removed lib.rs:1-2"). Optionally select the last chip on the
  first Backspace and remove it on the second, as Slack and GitHub do.

### KEY-3 major: F8 then Shift+F10, Ask agent attaches a selection, not the problem; the hover card's Ask agent cannot be reached by keyboard

- **Area:** ui/src/editor (`agent-context.ts`, `askAbout`; the lint
  tooltip)
- **Steps:** 1. Trust the project so `fake-lsp` runs; open `src/lib.rs`,
  Ctrl+E, add a line `// TODO fix overflow`. 2. Ctrl+Home, F8 (Go to next
  problem): the hover card "fake: TODO found [Ask agent]" shows and `TODO`
  is selected. 3. Tab: indents the code (the card's button is not in the
  tab order). 4. Shift+F10, `A` (typeahead lands on Ask agent), Enter.
- **Expected:** the problem goes to the task box (`lib.rs:4` with the
  problem icon and its message), as Ask agent on the card would do.
- **Actual:** the chip is a Selection: tooltip `src/lib.rs, lines 4`
  (screenshot `kb-f8-ask-agent-gives-selection.png`). The problem's message
  never reaches the agent. The chip label `lib.rs:4` is the same for a
  selection and a problem; only the small icon differs. The hover card
  stays on screen after focus moves to the Agents panel.
- **Why it matters for Oleh:** F8 is how he walks problems; the button on
  the card is mouse-only, so Shift+F10 is his only route, and it silently
  picks the wrong kind of item.
- **Suggested fix:** in `askAbout`, prefer the problem when the selection
  is exactly a diagnostic's range (which is what F8 leaves), or offer both
  in the menu ("Ask agent about the problem" / "about the selection").
  Make the card's actions reachable (CodeMirror's lint panel, Ctrl+Shift+M,
  or focusing the tooltip on a key). Fix the singular: "line 4".

### KEY-4 major: in a project with nothing open, typing `@` shows nothing

- **Area:** ui/src/agents (`ContextInput.svelte`, `options` / `open`)
- **Steps:** 1. Open the `small` project fresh (no recent files, no
  editor, no terminal). 2. Alt+N, type `Explain @`. 3. Also try the @
  button (Tab, Tab, Tab, Enter).
- **Expected:** the menu opens and lists files and folders (the spec says
  typing `@` opens the menu), or at least a hint row "Type to find a file
  or folder".
- **Actual:** `aria-expanded="false"`, no listbox (screenshot
  `kb-at-fresh-project-no-menu.png`). The menu appears only after a letter
  (`@lib`). With no query the menu offers only sources and the four recent
  files, so with none of those it stays closed.
- **Why it matters for Oleh:** a key that silently does nothing reads as
  "feature not there". The first time he tries @, it does nothing.
- **Suggested fix:** with an empty query and nothing else to offer, list
  the top-level folders and files (or the first rows of the project file
  list), or show a non-selectable hint row so the popup still opens.

### KEY-5 minor: Enter before the file list arrives starts the task with a literal `@query` and no chip

- **Area:** ui/src/agents (`ContextInput.svelte`, `AgentsPanel.svelte`
  `submitOnEnter`)
- **Steps:** 1. Open `monorepo`. 2. Alt+N, type `Fix @module-7` and Enter
  at once (33 ms in the script).
- **Expected:** Enter waits for or picks the first match while a query is
  being looked up, or does nothing while `@query` is unresolved.
- **Actual:** `agents.start {"prompt":"Fix @module-7"}` with no `context`;
  the task runs with the raw text (screenshot
  `kb-fast-enter-sends-raw-at.png`). The same happens with a typo that
  matches nothing: the menu closes and Enter starts the task. On this
  machine the list loads in about 120 ms, so a human typist only hits this
  on a cold, larger project, but nothing tells him the @ was not resolved.
- **Why it matters for Oleh:** he types ahead of the UI. An unresolved
  `@name` in the transcript looks like context was sent when it was not.
- **Suggested fix:** while the file list is loading and the caret is in an
  `@query`, keep the menu open with a "Loading files..." row and let Enter
  pick the first match once it arrives (or swallow it). With no match, show
  "No file matches" instead of closing, so Enter means "pick" only while
  the popup is visible.

### KEY-6 minor: the menu stays open after the caret leaves the `@query` or focus leaves the field

- **Area:** ui/src/agents (`ContextInput.svelte`, `refresh` is called only
  on input and click)
- **Steps:** 1. Type `Look at @lib`. 2. ArrowLeft eight times (caret in
  "Look") or Home. 3. Or: Shift+Tab with the menu open.
- **Expected:** the menu closes when the caret is no longer in the query,
  and when the field loses focus.
- **Actual:** after ArrowLeft x8 the menu is still open with
  `aria-activedescendant` set (screenshot
  `kb-stale-menu-after-arrowleft.png`); Enter there picks `lib.rs`, removes
  `@lib` and jumps the caret back to where `@` was. After Shift+Tab, focus
  is on "Hide" in Get started and the listbox still floats over the panel
  (`kb-menu-orphaned-after-shift-tab.png`) until he returns to the field.
- **Why it matters for Oleh:** he edits with arrows and Home/End; a popup
  that ignores caret moves makes Enter unpredictable.
- **Suggested fix:** call `refresh()` on `keyup` for caret keys (or on
  `selectionchange` while the field is focused), and close the menu on the
  field's `blur` (the options already use `pointerdown` with
  `preventDefault`, so clicking an option keeps focus).

### KEY-7 minor: the expanded state and chip changes are not exposed to assistive tech

- **Area:** ui/src/agents (`ContextInput.svelte`)
- **Steps:** 1. Type `Look at @lib`, ArrowDown. 2. Read the textarea's
  node in Chromium's accessibility tree (CDP
  `Accessibility.getFullAXTree`).
- **Expected:** the field announces that a list is open and which option
  is active; adding or removing a chip is announced.
- **Actual:** `activedescendant` and `controls` are exposed correctly
  (`agent-prompt-context-option-1`, the listbox with `selected=true` on the
  active option). But the node's role is `textbox` and there is no
  `expanded` property at all: `aria-expanded` is not valid on a textbox and
  Chromium drops it, and a `<textarea>` may not take `role="combobox"`.
  Nothing is announced when a chip is added, removed (KEY-2) or needs
  Include; the only live text is the error after Enter.
- **Why it matters for Oleh:** less for him than for screen reader users,
  but he also gets no feedback when Backspace eats a chip.
- **Suggested fix:** add one polite live region per box: "8 suggestions"
  when the menu opens, "Added lib.rs:1-2", "Removed strings.rs", ".env may
  hold secrets: choose Include". Keep `aria-activedescendant`; drop
  `aria-expanded` from the textarea or move the combobox semantics to a
  wrapper only if a screen reader test shows it helps.

### KEY-8 minor: Ask agent has no shortcut and is not in the palette; the terminal's is mouse-only

- **Area:** ui/src/editor (`context-menu.ts`), ui/src/terminal
  (`Terminals.svelte`)
- **Steps:** 1. In the editor, Shift+F10, then find Ask agent. 2. Ctrl+K,
  type `ask agent`. 3. In a terminal, look for Ask agent without a mouse.
- **Expected:** an "Ask agent about this" command in the palette with a
  shortcut, usable from the editor and the terminal.
- **Actual:** Ask agent is the 11th item of the editor menu (10 ArrowDowns;
  typing `A` jumps there, which helps). The palette has no match for
  `ask agent`. In the terminal the menu is pointer-only by design; the
  `@Terminal` item in the task box (selection, else last lines) covers the
  same need, and it worked.
- **Why it matters for Oleh:** "selection to agent" is a frequent action;
  three keys and a menu each time is slow, and the palette is where he
  looks for anything without a known key.
- **Suggested fix:** register an `agents.ask_about` command (editor:
  selection, else problem at cursor, else file; terminal: selection, else
  last lines) in the palette, with a default key that does not clash with
  browser shortcuts.

### KEY-9 minor: Ctrl+Enter with the menu open picks an item instead of starting; picking a duplicate does nothing visible

- **Area:** ui/src/agents (`ContextInput.svelte`, `keydown`;
  `context.svelte.ts`, `ContextDrafts.add`)
- **Steps:** 1. Ask agent on a problem (chip `lib.rs:4`). 2. Type
  `Fix @prob` (the same problem is the only option). 3. Ctrl+Enter.
- **Expected:** Ctrl+Enter starts the task (the help line says Mod+Enter
  starts it from anywhere in the box); or, if it picks, a duplicate pick
  says "Already added".
- **Actual:** Ctrl+Enter is treated as Enter: `@prob` is removed, no task
  starts, and since the item is already a chip nothing is added. The text
  just loses `@prob`.
- **Suggested fix:** let Ctrl/Cmd+Enter fall through to start while the
  menu is open (close the menu, keep the text). On a duplicate, briefly
  highlight the existing chip.

### KEY-10 polish: small rough edges

- **Area:** ui/src/agents (`ContextInput.svelte`, `AgentsPanel.svelte`)
- "Choose Include or remove the marked item first." stays under the box
  after Include was chosen (`kb-include-needed.png` shows it on first
  appearance; it is still there after Include and two more chips). Clear
  it on Include and on removal.
- `@term` lists `and-a-long-file-name-too.config.json` as the second match;
  `@src/` ranks Selection above the `src/` folder. For a query with `/`,
  rank folders and paths first.
- Long names (`flex: none`) push the menu's text past the window's right
  edge; the detail column disappears entirely
  (`kb-menu-orphaned-after-shift-tab.png`). Let the name shrink with an
  ellipsis and keep the menu inside the viewport.
- A full-width at sign (U+FF20, what Japanese and Chinese IMEs type for
  Shift+2) does not open the menu; `atQuery` matches only `@`. Accept both.
- On layouts without `@` on Shift+2 (Ukrainian, Russian), the only
  keyboard route without switching layouts is three Tabs to the @ button.
  A shortcut for "Add context" matched on `code` would help; this is not
  localization.

## What worked well

- Alt+N (Ovra remaps Mod+N in a browser tab) puts focus in the task box
  from the tree, the editor and the terminal.
- In the menu: ArrowDown/ArrowUp wrap, Enter and Tab pick, Shift+Tab is
  not swallowed, and Escape closes the menu without closing the panel and
  keeps `@query` as text; typing on after Escape does not reopen it.
  Menu keys and Backspace match on `event.code`; synthetic events with the
  right `code` are handled whatever `key` says.
- Picking removes exactly the typed `@query` and leaves the caret there,
  so typing continues naturally (`Explain ` then the next word).
- The editor's selection and problems, and the terminal's last lines, show
  up in the menu (`kb-menu-selection.png`); Shift+F10 with `A` typeahead
  reaches Ask agent; Ask agent focuses the task box with the chip.
- The amber chip blocks Enter with a clear message, and Include is the
  first Tab stop after the field.
- Speed with 15,250 files: first `@m` to a visible menu in 122 ms
  (including loading the file list); 13 to 17 ms from each keystroke to
  the next frame; the menu never has more than 8 rows.
- In Chromium, Enter during an IME composition is not taken by the menu or
  sent.
