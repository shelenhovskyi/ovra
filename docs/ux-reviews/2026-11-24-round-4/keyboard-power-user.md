# Keyboard power user (Oleh): Round 4 (2026-11-24)

Build: `81d16b1` on `main`, headless, real core (debug build with `fake-agent` and `fake-claude`), Chromium 158, 1280x800, Linux (Mod = Ctrl).
Scope: what landed since round 3 (`ca0af74..81d16b1`): terminals (focus in and out of xterm, keys the
terminal keeps or passes on, terminal tabs), the Agents panel (start with Ctrl+Enter, follow output,
interrupt, stop), approvals by keyboard (speed and safety), Trust project, the review inbox
(Alt+Shift+R in the browser: approve, reject, comment) and direct mode. Also a short re-check of
KEY-22, KEY-23 and KEY-24.
Fixtures: `small`, `monorepo` (seeded). The scripted fake agent was used for s1-s8 and s10-s13, and the real
Claude driver against `fake-claude --claude interrupted` for s9. Input was keyboard only (`page.keyboard`). After each step the
scripts logged `document.activeElement`, and a capture-phase window `keydown` spy recorded
`defaultPrevented`. There were 13 scripted runs (s1-s13). Scripts were scratch files and are not committed.

## Summary

Oleh can do almost all of the new work from the keyboard. Ctrl+`opens a shell that takes
type-ahead without losing keys. Ctrl+Enter starts an agent, and Alt+Shift+A then Y or N answers
its requests. Alt+Shift+R opens a properly modal review inbox. Of the round 3 items, KEY-23 and
KEY-24 are fixed and three of the four KEY-22 routes are fixed. The worst problem is a safety
problem. After he answers the last request, focus stays on the Approvals heading, and the next
request that arrives takes focus by itself. A sentence typed there more than 500 ms later allows
or denies that request with its`y`or`n`(KEY-29). Next come a transcript he cannot scroll from
the keyboard and that shows about 84 px of text (KEY-30), and three more buttons that drop focus
to`<body>` when pressed (Interrupt, Stop, Review and trust…) (KEY-31). The rest is friction:
global keys act behind the modal inbox, the terminal can only be left by hiding it, and a few
keys do nothing. Overall the new features are fast but less predictable than the editor and tree
he got used to in round 3.

## Regression table

| ID     | Round | Old severity | Status       | Evidence (round 4)                                                                                                                                                                                                                                                                                                                                                                                    |
| ------ | ----- | ------------ | ------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| KEY-22 | 3     | major        | Partly fixed | Ctrl+E from edit back to view now puts focus on `div.content` (s10). Go to file from the editor (Alt+P, `p42m33`, Enter) lands in the new file's `cm-content` (s10). Close project puts focus on the first project button, and the next Tab goes to the folder field (s10). Still open: Alt+W on the last tab gives `body` (s10 `Alt+W -> body`, `tabs []`). That is also KEY-25, which is unchanged. |
| KEY-23 | 3     | major        | Fixed        | Cold Alt+P, `p42m7` and an immediate Enter open `module-7.ts`. The palette closes, and focus goes to the tree (s10 `palette open=0 tabs=["module-7.ts"]`).                                                                                                                                                                                                                                            |
| KEY-24 | 3     | minor        | Fixed        | With a dirty `lib.rs`, Alt+W puts focus on Save, and Tab cycles Save → Don't save → Cancel → Save (s11). Esc from any of the buttons cancels and returns focus to the `lib.rs, unsaved changes` tab, and the next key goes to the buffer as expected (s11).                                                                                                                                           |

## New findings

### KEY-29 major: A request that arrives while focus sits on the Approvals heading takes focus, and a typed sentence answers it

- **Area:** `ui/src/approvals` (`Approvals.svelte`: `answer` leaves focus on the heading when the list empties, the `$effect` that focuses the first card when the heading has focus, and `armedAt`, which counts from arrival and not from focus)
- **Steps (s4, s5):**
  1. Seed `small` and `monorepo`. Open `demo-app` in one tab and `monorepo` in a second tab, and start "Fix the typo" in both with Ctrl+Enter.
  2. In the first tab, press Alt+Shift+A, then Y (waiting 600 ms each time) until the list says "Nothing waits for your approval." Focus is now on `h2#approvals-heading`. Nothing shows that a typed letter could still do something.
  3. In the second tab, send a reply ("go on"). The agent asks to run `git status`.
  4. In the first tab, still on the heading, type a sentence at about 11 keys per second, as he would after thinking he is back in the reply field.
- **Expected:** Typing never answers a request. A request is answered only by a key pressed on a card he moved to himself, or one that has had focus long enough for him to read it.
- **Actual:** The new card takes focus about 200 ms after it arrives. It is armed 500 ms after arrival, and the first `y` or `n` after that answers it. Typing "ok, please carry on with that" sent `{"method":"permissions.answer","params":{"id":"perm5","decision":"allow"}}` on the `y` of "carry". The other project's transcript shows `Run git status | Tool result | done` (s5). Typing "sure, go ahead and run the tests now" denied it on the `n` of "and" (s4: `Run git status | Tool failed`). The keydown spy shows `KeyY:true` and `KeyN:true` in the middle of plain typing. The card he allowed came from another project, and its label "Fix the typo asks to use Bash" was the same as the label of the card he had just answered (KEY-41).
- **Why it matters for Oleh:** he moves fast and trusts that letters he types are text. Here a stray letter runs a shell command he never read, in a project he was not looking at. Y and N are the right keys on a card he chose, but they must never be live because focus moved on its own.
- **Suggested fix:** After the last answer, return focus to where it was before Alt+Shift+A (`returnTo`), which `leave()` already does on Esc. Do not focus incoming cards automatically. Announce them in the `(n waiting)` count and the toast instead. If auto-focus stays, arm a card `ARM_MS` after it gets focus, not after it arrives. Also disarm it while letter keys keep arriving (for example, ignore Y and N within 1 s of any other printable key on that card).

### KEY-30 major: The agent transcript cannot be scrolled or read from the keyboard, and it shows about 84 px of text

- **Area:** `ui/src/agents` (`Transcript.svelte`, which is an `overflow: auto` `<ol role="log">` with no tabindex, and `AgentsPanel.svelte` layout)
- **Steps (s9, `--claude interrupted`):** 1. Start the ping task with Ctrl+Enter, allow it, interrupt it, reply "Never mind…", then stop. 2. Try to read the start of the transcript.
- **Expected:** He can focus the transcript (Tab, or a command), scroll it with arrows, PageUp/PageDown and Home/End, and see a useful amount of it.
- **Actual:** The transcript box is 84 px tall (`scrollTop 480, scrollHeight 564, clientHeight 84`), squeezed between "Total: …" and "Resume with a message" (`02-stopped.png`). The "Claude is ready" and "Project settings" cards keep their full height above it. It has `tabIndex -1`, and 15 Shift+Tabs from the reply field never reach it. Nothing scrolls it from the keyboard. The long "Tool failed" text of the interrupt is cut off and cannot be reached. Following live output works only while the box is scrolled to the end.
- **Why it matters for Oleh:** "follow the output" is how he decides what to answer. If the only way to read further back is the mouse wheel, he has to leave the keyboard for every agent task.
- **Suggested fix:** Give the log `tabindex="0"` and an accessible name, so it takes arrows, PageUp/PageDown and Home/End. Add a "Focus agent transcript" command with a key. Collapse the setup and trust cards to one line once Claude is ready and the trust choice is made, so the transcript gets the height.

### KEY-31 major: Interrupt, Stop and "Review and trust…" drop focus to `<body>`, and Trust moves it to the tree

This is the same class as KEY-22. These are new buttons that remove themselves when pressed.

- **Area:** `ui/src/agents` (Interrupt and Stop in `AgentsPanel.svelte`), `ui/src/agents/setup` (`ProjectTrust.svelte`), `ui/src/editor` (the last tab, which is KEY-22 and KEY-25)
- **Steps:**
  1. (s9) Allow the ping command, focus **Interrupt**, and press Enter. Later, focus **Stop** and press Enter.
  2. (s6) Tab from the tree to **Review and trust…** (3 Tabs) and press Enter. Then press Esc. Then focus **Trust project** and press Enter.
- **Expected:** Focus goes to the next sensible control: the reply field after Interrupt, the "Resume with a message" field after Stop, the first control of the trust review after "Review and trust…". It stays in the trust card (or its heading) after Trust. Esc closes the trust review.
- **Actual:** `Enter Interrupt -> body`, `Enter Stop -> body`, and `Enter Review and trust -> body`. The next Tab then starts again from the top of the page. Esc in the trust review does nothing (`Esc in trust review -> body`). Enter on **Trust project** moves focus to `div[role=tree]` in the sidebar, which is far from where he was. The trust panel has no palette command (searching `trust` in Ctrl+Shift+P lists only unrelated commands).
- **Why it matters for Oleh:** interrupting a runaway agent and then typing the correction is a two-second habit. Here it takes Enter, then several blind Tabs to find the reply field again.
- **Suggested fix:** Before removing a pressed button, move focus to its replacement. Interrupt goes to `#agent-reply`, Stop goes to `#agent-resume`, and "Review and trust…" goes to the review's first button (with Esc as Close). After trusting, focus the trust heading. A shared helper in `shell` that moves focus away from `<body>` would also cover KEY-22's last-tab case.

### KEY-32 minor: Global shortcuts act behind the modal review inbox

- **Area:** `ui/src/keys` (`installKeybindings` runs every command whatever the modal state), `ui/src/inbox`
- **Steps (s8, s1):** With the inbox open and focus in its diff, press Ctrl+`, type `echo typed-into-hidden-terminal`, and press Ctrl+` again. Then press Alt+Shift+A, then Alt+P.
- **Expected:** While an `aria-modal` dialog is open, focus stays inside it. Global commands are either ignored or close the dialog first.
- **Actual:** Ctrl+`puts focus in`xterm-helper-textarea` under the backdrop, and the typed text goes to the shell (`03-terminal-under-inbox.png`). Alt+Shift+A moves focus to `h2#approvals-heading` behind the dialog. Alt+P opens the palette on top of it. In s1, Alt+P from the start-screen inbox opened "Switch to project" over the empty inbox.
- **Why it matters for Oleh:** after a stray chord, he types into a pane he cannot see. Then Esc behaves differently depending on which layer has focus.
- **Suggested fix:** Have the keybinding layer skip commands while `[aria-modal="true"]` is open (except the dialog's own toggle). Alternatively, have such commands close the inbox first and restore its `returnTo`.

### KEY-33 minor: The terminal can only be left by hiding it, and its tabs are out of keyboard reach

- **Area:** `ui/src/terminal` (`Terminals.svelte`, `keys.ts`)
- **Steps (s1, s2):** 1. Ctrl+` (focus is in the xterm after 210 ms, and type-ahead is kept, s13). 2. Press Esc, Tab, Shift+Tab, F6. 3. Ctrl+Shift+` twice to make three terminals. 4. Try to switch to Terminal 1, or to move to the editor while keeping the terminal visible.
- **Expected:** One key moves focus out of the terminal while it stays visible (as with VS Code's Mod+1 for "focus editor", or F6 between regions). Another key switches terminal tabs from inside a terminal.
- **Actual:** Esc, Tab, Shift+Tab and F6 all stay in the xterm. Keeping them there is right for a shell, but it leaves Ctrl+`(which hides the panel and returns focus to the tree) as the only way out. The terminal tablist is not in the Tab order while the editor is in edit mode: seven Tabs from the tree stay in CodeMirror (s2). Once he reaches the tablist, its keys work: ArrowLeft, Home, End, Delete (closes, and focus goes to the next tab) and Enter (goes into the xterm) (s2). Alt+]/Alt+[ pressed in a terminal switch editor tabs, not terminals. After`exit`, the tab reads "Terminal 2 (exited)", focus stays in the dead xterm, and closing it needs the palette ("Close terminal" has no key).
- **Why it matters for Oleh:** tmux gives him one prefix to move between panes and windows. Here he has to hide the shell to look at a file and loses sight of its output.
- **Suggested fix:** Add "Focus editor" and "Focus terminal tabs" commands with keys, or make F6 cycle the regions: tree, main, terminal, agents. While the terminal has focus, let Alt+]/Alt+[ switch terminal tabs. Give "Close terminal" a key and focus the next terminal (or hide the panel) when a shell exits.

### KEY-34 minor: Alt+W in the terminal closes the editor tab, and its confirm takes focus from the shell

- **Area:** `ui/src/terminal` (`keys.ts` lets every non-Ctrl command through), `ui/src/editor`
- **Steps (s1, s2):** 1. Open `README.md`, then Ctrl+`into the shell and press Alt+W. 2. Make`lib.rs` dirty, Ctrl+` into the shell and press Alt+W.
- **Expected:** Alt+W in a terminal closes that terminal (asking if a process runs), or goes to the shell. It does not touch a tab he cannot see.
- **Actual:** In case 1, `README.md` closes behind the terminal (`tabs before ["README.md","Terminal 1"]`, `after ["Terminal 1"]`). In case 2, the "Save changes?" confirm appears and focus jumps from the shell to its **Save** button. The Enter he meant for a shell command would save the file. Alt+P from the shell also opens Go to file, so readline's `M-p` (history search) cannot be used.
- **Why it matters for Oleh:** in tmux, the close key closes the pane he is in. Here it acts on another pane and can save or discard a file there.
- **Suggested fix:** When the terminal has focus, map the close-tab command to the terminal tab, or leave Alt+W to the shell. Document the Alt+letter keys that the terminal gives up.

### KEY-35 minor: The first Tab on the start screen now opens the review inbox instead of the project

- **Area:** `ui/src/shell` (header order) and `ui/src/inbox` (the button shows with no project)
- **Steps (s1, s11):** Load the page with `small` seeded. Press Tab, then Enter (his round 3 habit).
- **Expected:** Tab reaches the first project, and Enter opens it (as in round 3).
- **Actual:** The first Tab lands on the header's **Reviews** button. Enter opens a modal "Review inbox" that says it cannot do anything without a project (`Enter (open project) -> h2#inbox-title`). It takes a second Tab to reach `demo-app`. On load, focus is still on `<body>` (KEY-28).
- **Why it matters for Oleh:** this changes the first two keys of every session, and the empty dialog looks like an error.
- **Suggested fix:** Hide or disable the Reviews button while no project is open. Also focus the first project on load (the KEY-28 fix), so that Tab order does not matter.

### KEY-36 minor: Ctrl+` does nothing when the panel is open with no terminal in it

- **Area:** `ui/src/terminal` (`terminal.toggle`: `focusActive()` with no active session)
- **Steps (s11):** 1. On the start screen, press Ctrl+`, then Ctrl+` again. 2. With the panel open in `demo-app`, switch to `monorepo` (Ctrl+Shift+P, `switch`, Enter, `mono`, Enter) and press Ctrl+`.
- **Expected:** A toggle hides the panel when there is nothing to focus. In a project, it starts a shell (as the first Ctrl+` does).
- **Actual:** In case 1, the panel shows "Open a project to use terminals.", and the second Ctrl+` leaves it open (`terminal hidden false`, focus `body`). In case 2, the panel says "No terminals in this project. New terminal (Ctrl+Shift+`)". Ctrl+` neither starts one nor hides the panel, and focus stays in the tree.
- **Suggested fix:** In `run`, if there is no active terminal, either create one (when a project is open) or hide the panel.

### KEY-37 minor: In the reject confirm, Esc closes the whole inbox, and Enter, Enter rejects

- **Area:** `ui/src/inbox` (`Inbox.svelte`: the confirm focuses `[data-confirm-reject]`, and `onDialogKeydown` closes on any Esc)
- **Steps (s7):** 1. In the inbox, focus **Reject…** and press Enter. 2. Press Esc. 3. Reopen, focus **Reject…** and press Enter twice.
- **Expected:** Esc backs out of the confirm and leaves focus on **Reject…**, the same way the first Esc in the comment field only clears the draft. A second Enter from key repeat does not throw away the agent's files.
- **Actual:** In step 2 the dialog closes, and focus goes to the agents reply field (`inbox open after Esc in confirm: 0`). After step 1, focus is on **Reject the changes**, so a second Enter (or a held Enter) rejects.
- **Suggested fix:** On Esc while `confirmingReject` is set, cancel the confirm and focus **Reject…**. Focus **Keep reviewing** when the confirm opens, or ignore Enter on the reject button for `ARM_MS`, as the approval cards do.

### KEY-38 minor: "Approve and commit" silently does nothing when the default message is empty

- **Area:** `ui/src/inbox` (`describe.ts` `defaultCommitMessage` returns `""` without a summary; Approve is `aria-disabled` without a reason)
- **Steps (s8):** In a direct review with no agent summary, Tab to "Commit the agent's files", press Space, Tab to the message field, press Enter, then Tab to **Approve and commit** and press Enter.
- **Expected:** A sensible default message. Enter in the message field approves, or a visible reason explains why the button does not.
- **Actual:** The message field is empty, and Enter in it does nothing. Enter on **Approve and commit** does nothing and gives no message. The item stays "Waiting for review" (`04-after-approve.png`). It also offers to "Commit the agent's files" when its only file is "Changed outside the agent".
- **Suggested fix:** Fall back to a message built from the session name. Make the decision area a `<form>` so that Enter submits. Say "Type a commit message" next to the disabled button.

### KEY-39 minor: The inbox puts the decision before the diff, and N/P only work once the diff has focus

- **Area:** `ui/src/inbox` (Tab order in `Inbox.svelte`), `ui/src/viewers` (`PatchView` keys)
- **Steps (s7, s8):** Open the inbox with Alt+Shift+R (focus goes to the item) and Tab through it. Then press N on the **Previous change** button.
- **Expected:** He reads before he decides. The order is item, then files, then diff, then the decision buttons and comment, and N/P jump between changes wherever focus is in the dialog (except in text fields).
- **Actual:** The order is item → commit checkbox → **Approve** → **Reject…** → comment → Send → file list → Previous change → Next change → diff table. The diff is the 9th stop. N and P work on the diff table (they moved to `+ added line 1: TOP`, then to line 12, then back, in s8), but they do nothing on the change buttons or the file list (s7).
- **Suggested fix:** Put the files and diff before the actions in DOM order, or move focus to the diff when an item is chosen. Handle N/P at dialog level when the target is not a text field.

### KEY-40 minor: Agent actions have no keys, and the keys that exist are not shown where he looks

- **Area:** `ui/src/agents`, `ui/src/approvals`, `ui/src/notify`
- **Steps (s3, s9, s12):** 1. Look for a key to reach the task field, to interrupt or to stop. 2. Read the Approvals hint and the approval toast.
- **Expected:** "New agent session", "Interrupt" and "Stop" have keys shown in the palette. The approval hint and toast name Alt+Shift+A.
- **Actual:** These three commands have no key. Esc and Ctrl+C in the reply field do nothing while the agent runs (s9). The hint says only "On a request: Y allows, N denies, Shift+N denies with a message." The toast offers a **Review** button that is 14 Tabs from the tree. Enter on it does focus the card, which is good (s12). From the tree, the prompt is 4 Tabs away after the setup and trust buttons (s1). With pending cards, there are more.
- **Suggested fix:** Give "New agent session" and "Interrupt" keys (avoid Ctrl+Shift+I, which the browser keeps). Consider Esc in an empty reply field as Interrupt while a turn runs. Add "Alt+Shift+A goes here" to the hint and the toast.

### KEY-41 polish: Approval cards and toasts from different projects look the same

- **Area:** `ui/src/approvals`, `ui/src/notify`
- **Steps (s4):** Start "Fix the typo" in `demo-app` and in `monorepo`.
- **Actual:** Two cards both say "Fix the typo asks to use Bash / Run ls -la" with no project name. One toast says "Fix the typo needs approval", and the other says "Session s2 needs approval" (`01-two-cards.png`).
- **Suggested fix:** Show the project name on each card, and use the same session name in toasts and cards.

### KEY-42 polish: Text typed within about 50 ms of Ctrl+E loses its first keys

- **Area:** `ui/src/editor` (the editor mounts after the mode change, and keys before that are lost)
- **Steps (s10, s7, s6):** Open `p2m2.ts` and press Ctrl+E, then type `abc` with no pause.
- **Actual:** The text becomes `cexport …` at 0 ms. It is correct with a 50, 100, 200 or 400 ms pause. In s6 and s7, `human edit` was saved as `uman edit` and `hello` as `ello`.
- **Suggested fix:** Buffer keys that arrive between Ctrl+E and the editor's mount and replay them, as the palette now does with an early Enter (KEY-23).

## What worked well

- The terminal keeps type-ahead. Ctrl+`followed straight away by`echo first-open⏎` runs, and so do reopening and Ctrl+Shift+` for a new terminal (s13). Ctrl+P, Ctrl+W, Ctrl+L, Ctrl+Q and Ctrl+E all reach the shell and are prevented in the browser (s1). Ctrl+` from a focused terminal hides the panel and gives focus back to where he came from (the tree, or the editor caret).
- Global keys he needs while in the shell do pass through. Ctrl+Shift+P and Alt+P open the palette, and Esc returns focus to the xterm (s1). Alt+Shift+A reaches approvals.
- Starting an agent works from the keyboard: Ctrl+Shift+P, `new agent`, Enter puts the caret in the task field, plain Enter adds a newline, Ctrl+Enter starts the session, and the field clears and keeps focus. A second task in the same project says clearly "another agent already works in this project's main checkout" (s12).
- Most of the approval design is right. A card never takes focus from the prompt, the reply field or the terminal, and typing `yes n ⏎` in the prompt while a card waits does nothing (s3). Enter and Space on a card do nothing. A double tap of Y (80 ms apart) answers one card only, and arrows move between cards (s4). Shift+N, a message and Ctrl+Enter send a denial with a reason.
- The review inbox is a real modal. Focus starts on the item, Tab wraps (Close → item), the first Esc in the comment field clears the draft, Esc closes and restores focus (to the tree, the editor or the reply field), and Ctrl+Enter sends a comment with a spoken confirmation (s7, s8).
- Round 3 fixes hold up. Cold Go to file with an immediate Enter works, Go to file from the editor and Ctrl+E back to view keep focus, Close project focuses the first project, and the dirty-close confirm is a proper modal.
