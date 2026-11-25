# Oleh, the keyboard power user: round 9

Build: `dd1b0d4` on `main`, headless, scripted fake agent (fake-lsp for Rust
files once the project is trusted), Chromium 158.0.8127.36, 1280x800.
Scope: Search or run (Ctrl+K), Settings (Ctrl+,) and its Keyboard pane, New
task (Alt+N in a browser tab), Reviews, approvals by keyboard (Y/N, "N more"),
focus and hover actions on list rows, Shift+F10 in the tree, Git and tabs,
preview and pinned tabs, Escape out of main-area views, shortcuts on a
Ukrainian layout. Fixtures: `small` (seeded).

## Summary

Oleh can do almost every task in scope without the mouse. Ctrl+K opens in
15 ms, every main-area view (Settings, Audit log, Reviews, a Git diff) closes
on Escape and gives focus back to exactly where he was, Shift+F10 works in the
tree, Git and tabs, and every global shortcut works on a Ukrainian layout. The
worst problems: once a Rust file has a language server, Ctrl+K stops opening
Search or run inside the editor, because the "Ctrl+K Ctrl+I" hover chord takes
the key. Every file opened from the keyboard is a preview tab that the next one
replaces, and he cannot keep it without pinning it. After the last request on
screen is answered, focus goes back into the task field, so the next Y he
presses for the follow-up request is typed as text. Overall the app feels fast
and consistent; what is left are a few traps where a key quietly does
something else.

Measured: Ctrl+K to a focused input in 15 ms. The Settings > Keyboard pane and
the palette's "Keyboard shortcuts" list show the same 33 commands. 14 synthetic
Ukrainian-layout shortcuts (Ctrl+K, Ctrl+,, Alt+Shift+R, Ctrl+`, Alt+],
Y and N, each sent with the Ukrainian layout's own character) all ran their commands.

## Findings

### KEY-1 major: Ctrl+K (Search or run) does nothing inside an editor that has a language server

- **Area:** `ui/src/editor` (`lsp/actions.ts`, `actionKeys`: `"Mod-k Mod-i"`), `ui/src/keys` (the chord also appears in the shortcut list)
- **Steps:** 1. Ctrl+Shift+P → "Review and trust this project" → Trust project (Rust files then get the fake language server). 2. Alt+P `lib.rs` Enter, Ctrl+E. 3. Press Ctrl+K. 4. Press Ctrl+I. 5. Press Ctrl+K again.
- **Expected:** Ctrl+K opens Search or run everywhere, as the title bar's "Search or run Ctrl+K" promises. Hover has a key that does not reuse the global one.
- **Actual:** Ctrl+K does not open the palette (`palette open: false`, focus stays in `Edit src/lib.rs`). CodeMirror treats it as the start of the `Mod-k Mod-i` chord and prevents the default action, and the global listener then skips it. Ctrl+I shows "fake hover". In the same file before trusting (no server), Ctrl+K opens the palette as usual, so the same key acts differently depending on whether a server runs. From the tree it always works. Screenshot `keys-ctrlk-swallowed-in-lsp-editor.png`.
- **Why it matters for Oleh:** Ctrl+K is the one shortcut the title bar teaches, and he presses it most often from the text. In the editor of a language with a server it silently stops working, and he has to Escape out of CodeMirror and Tab away before it works again.
- **Suggested fix:** move hover off the Ctrl+K prefix (VS Code's Ctrl+K Ctrl+I only works because VS Code has no global Ctrl+K). Use a single key such as Ctrl+Shift+I, or keep the chord only on Apple keyboards, where the palette is Cmd+K and the chord can use Ctrl. Add a test in `keys` that fails when a widget's keymap uses the first stroke of any registered global shortcut.

### KEY-2 major: no keyboard way to keep a preview tab; each file opened from keys replaces the last one

- **Area:** `ui/src/editor` (`open-files.svelte.ts` `makePermanent`, `commands.ts`), `ui/src/tree` (Enter), `ui/src/palette` (Enter on a file)
- **Steps:** 1. Ctrl+Shift+E, type `car`, Enter (Cargo.toml opens). 2. Alt+P `lib.rs` Enter. 3. Alt+P `main.rs` Enter. 4. Look for a way to keep a tab: press Enter again on the row, press Ctrl+E without typing, or search the palette for `keep`.
- **Expected:** a keyboard equivalent of the double click: Enter on a file that is already the preview keeps it, and a "Keep tab" command (VS Code: Ctrl+K Enter) exists. Opening a file from Go to file could also keep it, as in many editors.
- **Actual:** the tab strip always holds one tab: `["Cargo.toml, preview"]`, then `["lib.rs, preview"]`, then `["main.rs, preview"]`. A second Enter and Ctrl+E without an edit leave it a preview. `keep` finds no command. Only "Pin or unpin tab" or typing into the file keeps a tab, and pinning moves it to the front and outlives Close all. Screenshot `keys-preview-replaced.png`.
- **Why it matters for Oleh:** he opens three or four files by Alt+P and then cycles them with Alt+] / Alt+[. Today that loop has nothing to cycle through unless he pins every file, which is a different thing.
- **Suggested fix:** register `editor.keep_tab` ("Keep tab", listed in the palette and the tab's context menu), and make Enter on a tree row or palette item whose file is already the preview tab keep it. Add both to the Keyboard pane.

### KEY-3 major: after the last card on screen is answered, focus goes back to the task field and the next Y is typed there

- **Area:** `ui/src/approvals` (`answer`, the `heading?.focus()` / `leave()` path)
- **Steps:** 1. Alt+N, type a task, Ctrl+Enter. The fake agent asks "Run ls -la". 2. Alt+Shift+A, wait for the card to arm, press Y. 3. The agent's next request ("Edit README.md") arrives at once. Press Y again.
- **Expected:** either focus stays on the Approvals heading for a moment (where Y does nothing and a note says "A new request arrived: Alt+Shift+A to read it"), or the second Y is ignored with that note. It never ends up as text.
- **Actual:** after the first Y, focus is back in `TEXTAREA#agent-prompt` (where he was before Alt+Shift+A). The next Y, ArrowDown and N go into the prompt, which then reads "Third tasky". The new request is untouched (`Approvals (1 waiting)`). Screenshot `keys-y-typed-into-prompt.png`. With requests already on screen ("Plan and split", three workers) Y moves to the next card as it should, so the habit "Y, Y, Y" works in one case and types text in the other.
- **Why it matters for Oleh:** he answers by Y quickly and does not look between presses. Not focusing a request that was never seen is the right safety rule, but sending the key into a text field misleads him: he thinks he approved, and the task prompt now holds stray letters.
- **Suggested fix:** when the answered card was the last one and `returnTo` is a text field, keep focus on the Approvals heading until the next key-up plus about 1 s, or as long as a request arrives within that time. On the heading, Y or N shows "New request: press Alt+Shift+A to read it first" instead of passing through. Leave the "never focus an unseen request" rule as it is.

### KEY-4 minor: Ctrl+Enter still starts a task only from the text field (round 7 KEY-8, still open)

- **Area:** `ui/src/agents` (`AgentsPanel.svelte`)
- **Steps:** 1. Alt+N, type "Split this work". 2. Tab to the mode button, Enter, ArrowDown, Enter ("Plan and split"). 3. Ctrl+Enter.
- **Expected:** the task starts; picking the mode is the last step before starting.
- **Actual:** nothing happens and nothing is said (`Ctrl+Enter after picking mode: BUTTON "Plan and split"`; Approvals stays "Nothing waits"). Shift+Tab back to the field, then Ctrl+Enter, starts it.
- **Why it matters for Oleh:** an extra key on every split task, and a silent key.
- **Suggested fix:** handle Ctrl/Cmd+Enter on the whole compose form (`requestSubmit()`); with an empty prompt, focus it and say "Type a task first".

### KEY-5 minor: focus is on the page body after load and after closing Ctrl+K on the start screen (round 7 KEY-14, still open)

- **Area:** `ui/src/tree` (`ProjectList.svelte`), `ui/src/palette` (focus restore with no origin)
- **Steps:** 1. Load the app with one recent project. 2. Press Tab, then Enter. 3. Separately: Ctrl+K on the start screen, Escape.
- **Expected:** the first recent project has focus, so Enter opens it; Escape from the palette goes back there.
- **Actual:** `after load: BODY`. The first Tab goes to More (`BUTTON#more-button`) and Enter opens the More menu. Only the second Tab reaches `demo-app`. Escape from Ctrl+K also leaves focus on BODY. Screenshot `keys-load-focus-body.png`.
- **Why it matters for Oleh:** it is the first thing he does in every session.
- **Suggested fix:** focus the first recent project when the list loads and nothing else has focus (the empty-list case already focuses the folder field). When the palette closes without an origin, call the same `focusStartScreen()` as "Close project".

### KEY-6 minor: the Settings > Keyboard pane lists only palette commands, not the keys that work in context

- **Area:** `ui/src/settings` (`Settings.svelte`, keyboard pane), `ui/src/keys` (a way for panels to declare context keys), `ui/src/shell` (Audit log has no key)
- **Steps:** 1. Ctrl+, → Tab, Tab → ArrowDown to Keyboard. 2. Read the 33 rows and compare them with the keys the app answers to.
- **Expected:** one complete reference, grouped by place (Global, Editor, Tree, Git, Approvals, Reviews, Palette) and filterable; each key described once and correctly.
- **Actual:** the list is the 33 registered commands with keys, sorted by title, with no groups or filter. The same 33 appear in the palette's "Keyboard shortcuts" list. Missing: Y / N / Shift+N and ArrowDown on approvals; Shift+F10 and the Menu key; F2 and Delete in the tree; S / U / O / Delete / Enter on Git rows; N / P in diffs; Shift+F8 (previous problem); Ctrl+Enter (start task, commit, send comment); Escape (closes views); Ctrl+N/J/P/K in the palette; Alt+Shift+P (pick an element, only while the browser pane is open). F2 is listed only as "Language: Rename symbol", although in the tree it renames the file. Audit log still has no shortcut (round 7 KEY-9). "Ctrl+K Ctrl+I" is listed while it conflicts with Ctrl+K (KEY-1). The pane's line "Shortcuts follow the key's place, so they work on any layout" is not quite true: on a Latin layout, letters follow the label (QWERTZ's Y; `pressedKey` and approvals' `typed()`), and only non-Latin layouts and punctuation use the key's place. Screenshot `keys-settings-keyboard.png`.
- **Why it matters for Oleh:** he learns an app from its shortcut sheet. Half of what makes Ovra fast (Y/N, S/U on Git rows, Shift+F10) is only in hints scattered around the panels.
- **Suggested fix:** let panels register context keys (`{ keys, title, where }`, shown only, not bound globally) next to their commands, and render the pane grouped by `where`, with a filter field at the top. Reword the line to: "Letters follow your Latin layout; on other layouts and for punctuation, the key's place counts." Give Audit log a key.

### KEY-7 minor: focus does not show the hover actions on tabs and task rows; task rows have no context menu

- **Area:** `ui/src/editor` (`Tabs.svelte` styles), `ui/src/agents` (`AgentsPanel.svelte`, `.sessions`)
- **Steps:** 1. Pin Cargo.toml, open README.md (preview), focus the README tab and move with ArrowLeft and ArrowRight, with the mouse away. 2. In Agents → Tasks, Shift+Tab onto the row "First" (a finished task). Press Shift+F10, ArrowDown and Delete.
- **Expected:** a focused row shows the same actions as a hovered one (Pin on a tab, Close on a task), as Git rows already do (`.row:focus-within .slot`). Shift+F10 on a task row opens its menu, and arrows move between rows.
- **Actual:** the focused README tab has `pin` at opacity 0 (`README.md, preview focus=true pin=0`). The focused task row has `close=0`; it shows only when the Close button itself gets focus. Shift+F10 on a task row opens nothing, and ArrowDown does nothing. Every row and its Close button are separate Tab stops. Screenshot `keys-tab-focus-no-pin.png`.
- **Why it matters for Oleh:** he cannot see what a row offers without the mouse, and the task list is the one list in the app without arrows or a menu.
- **Suggested fix:** add `.tab:focus-within .pin` and `.sessions li:focus-within .close`. Give the task list a roving tabindex with ArrowUp/ArrowDown, Delete for Close, and a Shift+F10 menu (Open, Stop, Close).

### KEY-8 minor: tree typeahead does not reset on Enter

- **Area:** `ui/src/tree` (`FileTree.svelte`, `typeahead`)
- **Steps:** 1. Focus the tree, type `car`, press Enter (Cargo.toml opens). 2. Type `rea` within 0.7 s.
- **Expected:** a new search for `rea` selects README.md.
- **Actual:** the buffer continues as `carrea` and nothing matches: `selected after car,Enter,rea fast: Cargo.toml`. After a pause, `rea` works.
- **Why it matters for Oleh:** "type, Enter, type, Enter" is exactly how he walks a tree, and here it fails without a word.
- **Suggested fix:** reset `typed` on any key that is not a printable character (Enter, arrows, Home/End, Escape), as the ARIA tree pattern does.

### KEY-9 minor: opening a file from the palette while a view is open drops focus to the page body

- **Area:** `ui/src/shell` (`main-view.svelte.ts`), `ui/src/palette` (focus restore)
- **Steps:** 1. Ctrl+, (Settings, focus on its heading). 2. Alt+P `lib.rs` Enter. 3. Separately: open `lib.rs`, Ctrl+, then Alt+P `lib.rs` Enter again.
- **Expected:** the file shows and focus goes to the tree or the file, as it does when no view is open.
- **Actual:** case 2: Settings closes and focus is on BODY (`open file from palette while settings open: BODY`), because the palette returns focus to the Settings heading, which is gone by then. Case 3: picking the file that is already open does not close Settings at all; Ctrl+E then leaves focus on the Settings heading.
- **Why it matters for Oleh:** the next arrow or letter goes nowhere, and in case 3 the palette seems to have ignored him.
- **Suggested fix:** when the palette's return target is no longer connected, focus the main area's file (or the tree). Have `openFile` close the main view even when `path === openPath`.

### KEY-10 minor: after Keep changes, focus stays on the kept item

- **Area:** `ui/src/inbox` (`Inbox.svelte`)
- **Steps:** 1. Plan and split, allow the three workers (Y, Y, Y). 2. Alt+Shift+R, ArrowDown to `tests`, Tab ×3 to Keep changes, Enter.
- **Expected:** focus moves to the next item that waits for review, as approvals move to the next card.
- **Actual:** focus is on the heading of the kept item, `tests Kept`. He has to Shift+Tab back to the list and arrow down. Reaching Keep takes 3 Tabs from the list (Details, Commit the agent's files, Keep), and there is no single-key action like Y/N.
- **Why it matters for Oleh:** reviewing three workers is "keep, next, keep, next". Every step costs extra keys.
- **Suggested fix:** after Keep or Reject, focus the next waiting item in the list (or the list itself when none waits). Consider Ctrl+Enter on an item for "Keep changes", shown on the button the same way approvals show Y and N.

### KEY-11 minor: Ctrl+, does not close Settings, though the gear button and Reviews' shortcut toggle

- **Area:** `ui/src/settings` (`commands.ts`)
- **Steps:** 1. Ctrl+, 2. Ctrl+, again.
- **Expected:** the second press closes Settings and returns focus, as the rail's gear does and as Alt+Shift+R does for Reviews.
- **Actual:** Settings stays open (`settings visible after twice: true`) and focus goes back to its heading.
- **Why it matters for Oleh:** "the key that opened it closes it" holds for Reviews, the terminal, Servers and the browser pane, but not here.
- **Suggested fix:** run `mainView.toggle("settings")` from `settings.open` when Settings is already showing.

### KEY-12 minor: Alt+Shift+G lands in the commit message even when nothing is staged

- **Area:** `ui/src/git` (`focus.ts`)
- **Steps:** 1. Alt+Shift+G in `small` (2 changes, nothing staged). 2. Tab to the first changed file.
- **Expected:** with nothing staged, focus lands on the first changed file, where S stages it. The message field makes sense once something is staged.
- **Actual:** focus is in the commit message. It takes 5 Tabs (Amend, Commit, Unstage all, Stage all, then `src/main.rs`) to reach the first change. Screenshot `keys-git-open.png`.
- **Why it matters for Oleh:** "open Git, S, S, type message" becomes "open Git, five Tabs, S, ...".
- **Suggested fix:** focus the first changed file when nothing is staged; otherwise the message field.

### KEY-13 polish: Search or run ranks scattered file matches above an exact command, and menu labels mix cases

- **Area:** `ui/src/palette` (`search` mode ranking), `ui/src/tree` / `ui/src/editor` (menu label "Open Containing Folder")
- **Steps:** 1. Ctrl+K, type `set`. 2. Shift+F10 on a tree row.
- **Expected:** "Open settings" (a word that starts with `set`) comes first. Menu labels are in sentence case like the rest.
- **Actual:** three files come first: `a file with spaces.txt` (s-e-t scattered), `assets/logo.png` and the long config file. "Open settings" is fifth, because files always come before commands. The tree and tab menus say "Open Containing Folder" next to "Copy relative path". Screenshot `keys-ctrlk-set-ranking.png`.
- **Suggested fix:** in Search or run, rank across both sections by score and let a word-prefix command hit outrank scattered file hits (or show it at the top as "Best match"). Rename the item to "Open containing folder".

### KEY-14 polish: Escape in the Agents reply or task field does nothing

- **Area:** `ui/src/agents` (`AgentsPanel.svelte`)
- **Steps:** focus "Reply to Claude", press Escape.
- **Expected:** Escape leaves the field (to the task's heading or back to where he came from) and keeps the draft, as the Reviews comment field now does.
- **Actual:** focus stays in the field (`Esc in reply box: TEXTAREA#agent-reply`).
- **Suggested fix:** on Escape with no autocomplete open, keep the draft and focus the session heading. A second Escape returns to the element that had focus before Alt+N or Alt+Shift+A.

## Re-checked from round 7

- KEY-1 (search results all tabbable): fixed. Results use a roving tabindex (`SearchPanel.svelte`).
- KEY-2 (Ctrl+`on the Ukrainian layout): fixed. A synthetic`{key: "'", code: "Backquote", ctrlKey}` opens the terminal.
- KEY-6 (no key for Files): fixed. Ctrl+Shift+E "Show files" focuses the tree. The rail's tooltips still show no shortcuts.
- KEY-7 (early Y says nothing): fixed in the source (`answerByKey` sets the EARLY note).
- KEY-8 (Ctrl+Enter only from the text field): still open, now KEY-4.
- KEY-9 (panels without shortcuts): Rules, Network and Servers are now Settings panes; Audit log still has no key (in KEY-6).
- KEY-10 (Escape erases the comment draft): fixed in the source (first Escape moves to the item's heading and keeps the text).
- KEY-11 (Cancel returns to the tab): fixed in the source (focus goes back to where it was).
- KEY-12 (approvals match the physical key on QWERTZ): fixed (`typed()`); Y and N answer on a Ukrainian layout (Ukrainian characters on the same keys).
- KEY-13 (no Ctrl+N/J/P/K in the palette): fixed (`CTRL_MOVES`, by `code`).
- KEY-14 (focus on body after load): still open, now KEY-5.
- KEY-3, KEY-4, KEY-5 (browser pane Escape, Open a folder, header switch focus) were not re-run this round.

## What worked well

- Main-area views are consistent: Settings, Audit log, Reviews and a Git diff
  open with focus on their title, close on Escape (also when opened from
  Ctrl+K) and return focus to the exact tree row he left. Switching from
  Settings to Reviews and pressing Escape still goes back to that row.
- Ctrl+K is fast (15 ms) and doubles as project search on the start screen
  (projects plus "Open a folder…").
- Context menus by keyboard: Shift+F10 and the Menu key work on tree rows, Git
  rows and tabs. The menus wrap with arrows, support Home and End, and Escape
  returns focus to the row (the tree keeps its selection). Git rows show their
  keys in the menu (O, S, Delete) and under the list.
- Git rows: arrows move, focus shows the row's actions, Enter opens the diff,
  and Escape returns to the same row.
- Approvals with several requests: Alt+Shift+A focuses the oldest card,
  ArrowDown on the last card unfolds "2 more requests" and moves on, Y on each
  card moves to the next one, and the key caps (N, Y) appear only while the
  card has focus.
- The non-Latin layout is handled everywhere tested: Ctrl+K, Ctrl+,,
  Ctrl+Shift+E/F, Alt+P, Alt+Shift+R/G/A, Alt+N, Alt+W, Alt+] / Alt+[, Ctrl+E,
  Ctrl+0, Ctrl+` and Y/N on approvals all ran with Ukrainian characters.
- The Settings pane list uses a roving tabindex with arrows, Home and End, and
  the Keyboard pane shows the stand-ins a browser tab needs (Alt+W, Alt+P,
  Alt+N) rather than keys that cannot work there.
