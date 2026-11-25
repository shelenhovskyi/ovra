# Oleh, the keyboard power user: round 7

Build: `3b6b65a` on `main`, headless, scripted fake agent, Chromium 158.0.8127.36, 1280x800.
Scope: the whole app by keyboard after today's UX changes: the palette (files,
commands, switch project, Open a folder…), the header project switcher and the
More menu, the sidebar's Files/Search/Git views, the editor and its tabs, the
terminal, approvals, the Review inbox, the agents task form, and the Audit log,
Policies, Network, Servers and Browser panels. Fixtures: `small`, `monorepo`
(both seeded).

## Summary

Oleh can do every task in scope without the mouse: the header menus follow the
menu-button pattern (arrows wrap, Home/End, first-letter typeahead, Escape gives
focus back), every dialog keeps Tab inside and returns focus where it came from,
and approvals, the inbox and the task form all work from keys. The two worst
problems: Search in files makes every result a Tab stop (500 in `monorepo`; more
than 300 Tabs do not get out of the list), and Ctrl+` does nothing on a
Ukrainian layout. Most of the rest are focus surprises and silent keys:
"Open a folder…" closes the project at once, a header project switch leaves
focus in the header, an early Y on an approval does nothing and says nothing,
and the browser pane still ignores Escape. Overall the app now feels fast and
predictable; what is left is consistency.

Measured: Alt+P shows file rows in 37 ms; opening a project from the start
screen takes 77 ms to a focused tree; typing `cargo` and Enter straight after
Alt+P opens the right file. The command palette lists 48 commands, 24 with a
shortcut.

## Findings

### KEY-1 major: every search result is a Tab stop, and arrows do not reach the results from the query

- **Area:** `ui/src/search`
- **Steps:** 1. Open `monorepo`. 2. Ctrl+Shift+F, type `export`, Enter (500 results). 3. Press ArrowDown in the query. 4. Press Tab until a result is focused. 5. From the first result, press Tab until focus leaves the sidebar.
- **Expected:** ArrowDown (or Enter on a finished search) moves from the query to the first result. The result list is one Tab stop (roving tabindex), with arrows, Home and End inside it, so one Tab leaves it.
- **Actual:** ArrowDown in the query does nothing (`ArrowDown in query: input "Search text"`). It takes 5 Tabs to reach the first result (past Match case, Whole word, Regex, Include/exclude). All 500 result buttons are tabbable (`results {"results":500,"tabbable":500}`), and 300 Tabs from the first result are still inside the list (`Tab presses to leave the results: 301`, still on `pkg-006/src/module-0.ts`). Arrows inside the list do work, and Enter opens the file at the line with focus in the editor.
- **Why it matters for Oleh:** in Neovim or VS Code, search, then Down, then Enter is three keys. Here it is five Tabs to get in and no practical way to Tab out; Shift+Tab back to the query is the only way out.
- **Suggested fix:** ArrowDown in the query focuses the first result. Give result buttons `tabindex="-1"` except the current one (roving tabindex, as the tree and the inbox lists do). Escape in the results returns to the query. Show a short "↓ to results" hint under the status line.

### KEY-2 major: Ctrl+` (toggle terminal) does nothing on a Ukrainian layout

- **Area:** `ui/src/keys` (`shortcut.ts`, `pressedKey`), which `ui/src/terminal/keys.ts` also uses
- **Steps:** 1. Switch to the Ukrainian layout. On it the Backquote key types an apostrophe (`key: "'"`, `code: "Backquote"`). 2. With focus in the tree, press Ctrl+`. (Scripted as a synthetic keydown with those values. The same event with the Russian layout's Cyrillic letter on that key works.)
- **Expected:** the terminal opens, as it does on a Latin layout or a Russian one.
- **Actual:** nothing happens and the event is not handled (`UA Ctrl+' (Backquote): prevented false`). `pressedKey` uses `event.key` whenever it is printable ASCII and only falls back to `code` for non-ASCII characters. The Ukrainian apostrophe is ASCII, so the shortcut looks for "`" and finds "'". The same rule breaks any punctuation shortcut on a layout where that key types other ASCII punctuation (the Russian layout's Slash key types `.`). Ctrl+Shift+` works, because Shift already takes the physical key.
- **Why it matters for Oleh:** he switches between layouts all day. The terminal shortcut dies without a word as soon as he is on Ukrainian, which is exactly what the `KeyboardEvent.code` rule in CLAUDE.md is meant to prevent.
- **Suggested fix:** for punctuation keys, always match on `PUNCTUATION[event.code]` (with or without Shift), and keep `event.key` only for letters on Latin layouts. Add unit cases for `{key: "'", code: "Backquote", ctrlKey: true}` and `{key: ".", code: "Slash"}` to `shortcut.test.ts`.

### KEY-3 minor: Escape still does not hide the agent browser pane

- **Area:** `ui/src/browser`
- **Steps:** 1. Ctrl+Shift+B (or More → Agent browser). Focus goes to the "Browser" heading of the pane. 2. Press Escape.
- **Expected:** the pane hides and focus goes back to where it came from, as every More-menu dialog does.
- **Actual:** nothing happens; focus stays on the heading (`Control+Shift+b then Esc: h2 "Browser"`). Only Ctrl+Shift+B again or Tab to "Hide" closes it. This is KEY-7 of the servers-browser round, still open. Screenshot `keys-browser-pane-esc.png`.
- **Why it matters for Oleh:** it is the only panel in the More menu that ignores Escape. Since today's change it opens from the same menu as four modal dialogs, so the difference shows more.
- **Suggested fix:** handle Escape on the pane (not inside the page view while controlling) like `toggle()`, and return focus to the element that opened it.

### KEY-4 minor: "Open a folder…" closes the open project at once, and Escape does not bring it back

- **Area:** `ui/src/shell` (`open-folder.ts`), `ui/src/palette` (projects mode)
- **Steps:** 1. In `demo-app` with `Cargo.toml` open, Ctrl+Shift+P → "Switch project" → type `open` → Enter on "Open a folder…" (or the header switcher → Open a folder…). 2. Change your mind and press Escape.
- **Expected:** a folder field or prompt over the current project; Escape cancels and leaves `demo-app` as it was.
- **Actual:** in headless mode the project is closed straight away and focus goes to the start screen's folder field (`project-name No project open`). Escape in the field does nothing. Getting back means picking `demo-app` from the recent list again. Screenshot `keys-open-folder-closes-project.png`.
- **Why it matters for Oleh:** a command that looks like it only asks a question has already changed the window. Exploring a menu should never cost a re-open.
- **Suggested fix:** in headless mode, open a small path prompt (the start screen's field and its completion, in a popover or the palette itself: "Type a folder path") and switch only once a folder is chosen. If leaving for the start screen stays, then Escape in the field with nothing typed reopens the previous project, and the menu item says so: "Open a folder… (closes demo-app)".

### KEY-5 minor: focus after switching project depends on where you switched from

- **Area:** `ui/src/shell` (`ProjectSwitcher.svelte`)
- **Steps:** 1. Header: focus the project button, ArrowDown, Enter on `monorepo`. 2. Compare with Ctrl+Shift+P → Switch project → `mono` → Enter.
- **Expected:** both land in the new project's tree, ready for arrows.
- **Actual:** the palette puts focus on the tree. The header menu leaves it on the header button (`focus after switching from header menu: button#project-name`), so the next arrow opens the menu again instead of moving in the tree.
- **Why it matters for Oleh:** the same action must leave him in the same place, or he has to look before every key.
- **Suggested fix:** after a successful `switchTo`, close the menu without refocusing the button and focus the tree, as the palette path does (`FileTree` already focuses itself on a project change from the palette).

### KEY-6 minor: no keyboard way back to the Files view; the sidebar tabs are three Tab stops with no hint

- **Area:** `ui/src/shell` (sidebar tabs), `ui/src/tree` (a focus command)
- **Steps:** 1. Alt+Shift+G (Git view). 2. Try to get back to the file tree without the mouse. 3. Tab to the Files/Search/Git buttons and press ArrowRight.
- **Expected:** a shortcut that shows Files and focuses the tree (VS Code's Ctrl+Shift+E), listed in the palette. The three view buttons behave as a tablist (arrows move, one Tab stop) and show their shortcuts in a tooltip.
- **Actual:** Search has Ctrl+Shift+F and Git has Alt+Shift+G, but Files has nothing. "Reveal open file in the tree" brings it back only when a file is open. Searching the palette for `files`, `focus` or `sidebar` finds nothing that does it. ArrowRight on the buttons does nothing; they are three Tab stops on every pass through the page. None of them has a title or `aria-keyshortcuts`. The task field ("New task for Claude") also has no shortcut.
- **Why it matters for Oleh:** moving between panels by keys is the core of his goal. Today two of the three views have a key and the main one does not.
- **Suggested fix:** register `tree.focus` ("Show files", Mod+Shift+E) that sets `workspace.sidebar = "files"` and focuses the tree. Make the three buttons a `role="tablist"` with arrow keys and a roving tabindex, with titles like "Search (Ctrl+Shift+F)". Consider a shortcut for "New task for Claude".

### KEY-7 minor: Y or N pressed before an approval is armed does nothing and says nothing

- **Area:** `ui/src/approvals`
- **Steps:** 1. Start a task; when "Run ls -la" arrives, press Alt+Shift+A and then Y at once (within 500 ms).
- **Expected:** the same note a click gets: "Ready in a moment — read the request first."
- **Actual:** the press is ignored without a word (`Y immediately: pending 1`, no note on the card). Only the dimmed buttons hint at it. A click in the same window shows the note (`answerByClick` sets `said`, `answerByKey` returns silently). Screenshot `keys-approval-early-y.png`.
- **Why it matters for Oleh:** he answers by Y fast. A key that silently does nothing makes him press again, and he cannot tell "too early" from "not focused".
- **Suggested fix:** in `answerByKey`, when `!armed(request.id)` and the key is not a repeat, set `said = { id, text: EARLY }` as the click path does.

### KEY-8 minor: Ctrl+Enter starts the task only from the text field

- **Area:** `ui/src/agents` (`AgentsPanel.svelte`)
- **Steps:** 1. Type a task. 2. Tab to the mode radios and pick "Plan and split" with ArrowRight. 3. Press Ctrl+Enter. 4. Separately, press Ctrl+Enter with an empty task field.
- **Expected:** Ctrl+Enter starts the task from anywhere in the form, as the hint "Ctrl+Enter starts." says. With an empty field, a short "Type a task first".
- **Actual:** on the radios Ctrl+Enter does nothing (`Ctrl+Enter on radio started? sessions 0`); he has to Shift+Tab back to the text field first. With an empty field nothing happens and nothing is said.
- **Why it matters for Oleh:** picking the mode is the last step before starting, so it is exactly where he presses Ctrl+Enter.
- **Suggested fix:** handle Ctrl/Cmd+Enter on the whole `<form>` (keydown on the form, `requestSubmit()`); when the prompt is empty, focus it and show the hint in the form's status line.

### KEY-9 minor: three of five More-menu panels have no shortcut; headless shortcut families are mixed

- **Area:** `ui/src/audit`, `ui/src/policy`, `ui/src/network` (register keys), `ui/src/keys` (`BROWSER_SAFE`)
- **Steps:** 1. Open the More menu. 2. Look at the palette's list of shortcuts.
- **Expected:** every panel in the menu has a shortcut, and the shortcuts follow one pattern.
- **Actual:** Servers (Ctrl+Shift+S) and Agent browser (Ctrl+Shift+B) have one; Audit log, Project policies and Network and secrets do not. In a browser tab, panel shortcuts are split between Ctrl+Shift (F, S, B, P) and Alt+Shift (A, R, G) depending on what Chromium keeps, so he cannot guess the modifier.
- **Why it matters for Oleh:** the security panels are the ones he opens while reviewing an agent; the palette works, but it is three extra keys every time.
- **Suggested fix:** give the three panels shortcuts (for example Mod+Shift+L for the audit log). In headless mode, consider moving all panel toggles to one modifier family (Alt+Shift) so the rule is "panels are Alt+Shift+letter in the browser".

### KEY-10 minor: Escape in the inbox's comment field erases the draft

- **Area:** `ui/src/inbox` (`onCommentKeydown`)
- **Steps:** 1. Alt+Shift+R, Tab to "Comment to the agent", type "please rename the file". 2. Press Escape, intending to leave the field.
- **Expected:** Escape leaves the field (or closes the inbox) and the draft stays, or at least Ctrl+Z brings it back.
- **Actual:** the text is gone (`comment after Esc: ''`) and focus stays in the empty field. The value is cleared from code, so the textarea's own undo cannot restore it.
- **Why it matters for Oleh:** Escape is his "get me out" key everywhere else; here it deletes his words.
- **Suggested fix:** keep the draft per review item. The first Escape blurs the field to the item's heading; the next closes the inbox. Or ask before discarding a non-empty draft.

### KEY-11 minor: Cancel in "Save changes before closing?" returns focus to the tab, not to the text

- **Area:** `ui/src/editor` (`Tabs.svelte`, `focusTab` after `cancel`)
- **Steps:** 1. Open `Cargo.toml`, Ctrl+E, type `xyz`. 2. Alt+W. The prompt opens with focus on Save. 3. Press Escape.
- **Expected:** focus goes back into the editor at the cursor, where he was typing.
- **Actual:** focus is on the tab button `Cargo.toml, unsaved changes`. Typing goes nowhere until he presses Tab or Enter.
- **Why it matters for Oleh:** Cancel means "never mind, keep typing"; one more key and a moment of "where am I".
- **Suggested fix:** on cancel, restore focus to the element that had it before the prompt (the editor's content), falling back to the tab.

### KEY-12 minor: approvals match the physical key, so on QWERTZ the key labelled Z allows

- **Area:** `ui/src/approvals` (`onKeydown`, `case "KeyY"` / `"KeyN"`)
- **Steps:** (from the source, not run) On a German QWERTZ layout, the key labelled Z has `code: "KeyY"`; the key labelled Y has `code: "KeyZ"`.
- **Expected:** the key labelled Y (the hint and the `<kbd>Y</kbd>` on the button) allows; a non-Latin layout falls back to the physical key, as the global shortcuts already do.
- **Actual:** approvals match only `event.code`, so on QWERTZ "Z" allows and "Y" does nothing. The menus (`menu.ts`) and global shortcuts (`shortcut.ts`) match the typed Latin letter first and fall back to `code`. That is three rules in one app.
- **Why it matters for Oleh:** an allow on a key whose label says something else is the wrong kind of surprise for a permission prompt, and the inconsistency makes him unsure which rule a key follows.
- **Suggested fix:** use the same helper as `menu.ts` (`typed()`): the Latin letter from `event.key`, else the physical letter from `event.code`. Share it from `ui/src/keys`.

### KEY-13 minor: the palette has no Ctrl+N/Ctrl+P (or Ctrl+J/K), PageDown, Home or End

- **Area:** `ui/src/palette` (`Palette.svelte`, `onkeydown`)
- **Steps:** 1. Alt+P, type `readme`. 2. Press Ctrl+N, Ctrl+J, PageDown.
- **Expected:** Ctrl+N/Ctrl+P (Emacs, VS Code, fzf) or Ctrl+J/Ctrl+K (Vim users' fzf) move the selection; PageDown/PageUp move by a page in a long list.
- **Actual:** none of them moves the selection (`Ctrl+N in palette: selected …and-a-long-file-name-too.config.json`, unchanged). Only ArrowUp and ArrowDown work (they wrap, which is good).
- **Why it matters for Oleh:** his hands stay on the home row; reaching for the arrows on every palette use is the friction he notices most.
- **Suggested fix:** map Ctrl+N/Ctrl+J to down and Ctrl+P/Ctrl+K to up (by `event.code`), PageDown/PageUp to ±10, and leave Home/End to the input.

### KEY-14 minor: on load with recent projects, focus is on the page body

- **Area:** `ui/src/tree` (`ProjectList.svelte`)
- **Steps:** 1. Load the app with two recent projects and none open. 2. Press Tab.
- **Expected:** the first recent project is focused, so Enter opens it (the empty-list case already focuses the folder field).
- **Actual:** `focus after load: BODY`. Tab goes to Reviews, then More, and only the third Tab reaches `demo-app`. Opening a project from load takes four keys instead of one.
- **Why it matters for Oleh:** it is the first thing he does every session.
- **Suggested fix:** when the list loads with projects and nothing else has focus, focus the first project button (the same rule as the empty case, `ProjectList.svelte` line 48).

### KEY-15 polish: two focus-ring styles and some unstyled light buttons

- **Area:** `ui/src/shell` (sidebar tabs), `ui/src/editor` (`Tabs.svelte`, Save), `ui/src/viewers` (`PatchView.svelte`), `ui/src/search` (option toggles)
- **Steps:** Tab through the sidebar buttons, the editor tabs, Collapse all and the search toggles; open a diff in the inbox; enter edit mode.
- **Expected:** one focus ring everywhere (the 2px `#7aa2ff` outline used by the header, dialogs, approvals and agents); dark buttons that match the theme.
- **Actual:** these controls show Chromium's default thin white ring (`outline auto 1px`). It is visible, but it differs from the rest of the app. "Previous change" / "Next change" in the diff and "Save" in the editor are light grey/white native buttons (`bg rgb(239, 239, 239)`) in the dark UI, and the diff buttons do not show their N/P keys (only `aria-keyshortcuts`). Screenshot `keys-inbox-diff-buttons.png`.
- **Suggested fix:** a shared `:focus-visible` token in the shell's global styles; style the diff and Save buttons like the other toolbar buttons and add the key as a `<kbd>` ("Next change N").

### KEY-16 polish: raw "Mod" in a command title, and a noisy fuzzy match

- **Area:** `ui/src/editor` (`commands.ts` LSP titles), `ui/src/palette` (`fuzzy.ts`)
- **Steps:** 1. Ctrl+Shift+P. 2. Read the list; type `audit`.
- **Expected:** the chord is shown in the shortcut column as `Ctrl+K Ctrl+I`; `audit` finds only "Audit log".
- **Actual:** the title reads "Language: Show hover at cursor (Mod+K Mod+I)", with "Mod" spelled out in the title instead of the key column. `audit` also lists "Language: Go to definition" second (scattered letters a-u-d-i-t).
- **Suggested fix:** let `keys` hold a chord and format it in the key column; raise the fuzzy threshold for scattered single-letter matches across word gaps.

## Re-checked from earlier rounds

- servers-browser KEY-7 (Escape does not hide the browser pane): still open, now KEY-3.
- servers-browser KEY-10 (header buttons do not show shortcuts): partly fixed. The More menu shows each panel's shortcut, and Reviews has the title "Review inbox (Alt+Shift+R)". The sidebar view buttons still show none (KEY-6).

## What worked well

- Header menus are by the book: Enter, Space, ArrowDown open on the first item and ArrowUp on the last; arrows wrap; Home/End; first-letter typeahead (including recent project names); Escape returns focus to the button; Tab closes and moves on. Shortcuts are shown in the More menu with a one-line purpose for each panel.
- Every dialog (Audit log, Policies, Network, Servers, Review inbox) opens with focus on its heading, keeps Tab inside, closes on Escape and returns focus to the exact origin (the More button, or the tree when opened by shortcut). Ctrl+Shift+S toggles Servers closed again from inside it.
- The palette is fast (37 ms to rows) and reliable: typed-then-Enter opens the right file, arrows wrap, each reopen clears the query, Escape restores focus, and "Switch project" lists the projects plus "Open a folder…" with paths.
- Non-Latin input works for letter shortcuts: Alt+P and Ctrl+Shift+P open the palette when the P key types its Ukrainian Cyrillic letter, Ctrl+` toggles the terminal on the Russian layout (a Cyrillic letter on that key), and the menus' typeahead falls back to the physical key.
- Approvals are safe and still fast by keys: Alt+Shift+A focuses the oldest request, Y/N answer after the arming delay, a new request never steals focus, ArrowDown from the heading reaches the next, Shift+N opens a note that Ctrl+Enter sends, and Escape returns to where he was. With nothing waiting, Alt+Shift+A shows "Nothing waits for your approval." instead of failing silently.
- The Review inbox opens on the item to review, arrows move between items, and Alt+Shift+A from inside it closes it and goes to approvals.
- Editor and tabs: Ctrl+E puts focus in the text, Escape then Tab leaves CodeMirror, Delete on a focused tab closes it, the dirty-close prompt opens on Save, and Ctrl+` / Ctrl+Shift+` toggle and add terminals with focus in the shell. Alt+P works from inside the terminal and Escape returns there.
