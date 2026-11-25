# Nadia, the newcomer: round 8 (2026-11-25)

Build: 2236c61 (main), headless with the scripted fake agent, Playwright Chromium 158.0.8127.36, 1280x800 (also 1024x700), light and dark (`prefers-color-scheme`).
Scope: the first session in the landed Quiet design: start screen, opening a project, the title bar (Search or run, Reviews, Agents toggle, More), file tabs (preview and pinned), the Agents column (Get started steps, the "Give Claude a task" composer, the mode menu, task rows with progress, transcript cards, approval cards with Deny / Allow once), closing a task, toasts. Fixtures: `small` (as demo-app; seeded in most runs, typed in by hand in one), `empty`.

Runs: nine scripts. Screenshots are in `/mnt/project-files/ovra/screenshots/round-8/` as `newcomer-*.png`. Words were counted over every visible text node. Contrast was computed from the computed colour, the effective opacity and the nearest opaque background.

Not checked: the "Done · Changed README.md / Show changes" card, because the fake agent's Edit does not change the file (the fixture README has no "teh"). Its transcript still shows "Edited README.md +1 −1".

## Summary

Nadia could do the whole first session without help. She opened a project, read files, started a one-agent task and a split task, answered approvals, opened Reviews and closed a task. The Quiet design now reads as one product. The start screen says what Ovra is for, the Agents header no longer wraps, Get started tells her the next step, the mode menu explains both options, and closing a task now asks first. The worst problems: "New task" does nothing while the Agents column is folded (NEW-16). When approvals are waiting, the Agents column has no room left: the card is cut, the transcript shrinks to one half-clipped line, and the composer hint is cut off (NEW-17). The Y and N key caps on the approval card type into the reply box right after Start (NEW-18). The column still says the same thing several times, in particular "Review" and "No sandbox" (NEW-19, NEW-20).

New: 0 blocker, 2 major, 7 minor, 3 polish.

Re-check of the Quiet round (`2026-11-25-quiet/newcomer.md`):

| ID     | State            | Note                                                                                                                                                                                                                         |
| ------ | ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NEW-1  | **partly fixed** | The header is one line ("Agents 1 of 3 ▬▬▬ +"). The approval card is still cut by the Agents section, at 1024x700 too (see NEW-17).                                                                                          |
| NEW-2  | **partly fixed** | The × asks first: "Close this task and its 3 workers? Their changes stay in Reviews." The × has its own column and no longer covers the Review pill. I did not re-check what happens to the workers' statuses after closing. |
| NEW-3  | **fixed**        | "Open a project", one line on what Ovra does, Recent with initials, Claude status in a second column, no rail.                                                                                                               |
| NEW-4  | **partly fixed** | The inbox groups items under the task and opens on the first one (lint). Still 440 words, the title still reads "lint (task: Add notes for the project)", and "Also changed in another open review" is on every file.        |
| NEW-5  | **fixed**        | No trust card for a project without settings of its own. The empty-prompt Start reads as disabled, with the tooltip "Type a task first" (5.42:1).                                                                            |
| NEW-6  | **open**         | "Change" on a ready Claude still opens a filled "Check again", "How to fix" and a path field with a cut placeholder (`newcomer-41-claude-change.png`).                                                                       |
| NEW-7  | **mostly fixed** | The cards are grouped under "Task: Add notes for the project · demo-app" with one "No sandbox" chip. There is one toast with a count, and none while a card is in sight. 284 words instead of 367.                           |
| NEW-10 | **open**         | A Done task still shows Stop next to Close (`newcomer-24-after-allow-1.png`, `newcomer-36-task-open.png`).                                                                                                                   |
| NEW-13 | **fixed**        | "Search or run Ctrl+K" searches files and commands; the placeholder is "Search files, or type > for commands".                                                                                                               |
| NEW-14 | **partly fixed** | The error replaces the hint, and "Folder not found: /no/such/folder" and "That is not a folder…" are clear. `~/demo-app` still gets "Use the full path…".                                                                    |

## Findings

### NEW-16 major: "New task" does nothing while the Agents column is folded

- **Area:** ui/src/agents (AgentsPanel.svelte `newTask`), ui/src/shell (unfolding a panel)
- **Steps:** 1. Open demo-app. 2. Click the Agents toggle in the title bar to hide the column, to get more room for a file. 3. Click "New task" in the empty editor's list, or choose "New task for Claude" in Search or run, or press Ctrl+N.
- **Expected:** The Agents column comes back with the cursor in "Give Claude a task", as "Go to pending approvals" now does (commit 7a9053f).
- **Actual:** Nothing happens in all three cases. The prompt stays hidden and focus stays on the button (`newcomer-63-newtask-folded.png`, log: `after palette: prompt visible=false`, `after empty-state New task: prompt visible=false`, `after Ctrl+N: prompt visible=false`). The only way back is the robot icon in the 48 px strip, which has no label.
- **Why it matters for Nadia:** "New task" is the first item in the list that tells her what to do. A button that does nothing tells her the app is broken, and she has no clue that the folded column is the reason.
- **Suggested fix:** Make `newTask` unfold the Agents column (the same call the approvals shortcut uses) before it focuses the prompt. Add an e2e step: fold Agents, then run the command.

### NEW-17 major: While approvals wait, the Agents column has no room: the card is cut, the transcript is one clipped line, and the composer hint is cut off

- **Area:** ui/src/agents (AgentsPanel.svelte layout: `.info`, the transcript, the composer), ui/src/approvals (Approvals.svelte height), ui/src/shell (Shell.svelte agents column)
- **Steps:** 1. At 1280x800, start "Fix the typo in the README". 2. Look at the column while the first card waits. 3. Do the same at 1024x700. 4. Start "Add notes for the project" with Plan and split, then click "Tasks".
- **Expected:** The approval card is shown whole, the transcript keeps at least a few lines so she sees what Claude is doing, and nothing is cut off in the middle of a line.
- **Actual:**
  - At 1280x800 the transcript gets about 60 px. Its top line is cut through the middle of the letters, above "README.md." (`newcomer-22-first-approval.png`, `newcomer-51-after-msg-deny.png`).
  - At 1024x700 the card's "Deny with a message…" is cut by the Agents section, and the transcript shows only "You" under a floating "Jump to latest" (`newcomer-45-after-deny.png`).
  - With three worker approvals, the task list shows only the task row: the three worker rows with their "Approve" pills sit under the composer. The composer's own hint, "Up to 3 agents on their own copies, without a sandbox. You review first.", is cut in half at the bottom of its box (`newcomer-32-split-list.png`).
  - In a coordinator's view, "3 changes wait for your review. / Review" is a white box laid over the transcript, which hides "Started worker lint" behind it (`newcomer-36-task-open.png`).
- **Why it matters for Nadia:** This is the moment she must decide, and she cannot see what led to the request. Text cut through the middle of the letters looks broken.
- **Suggested fix:** Give the Approvals block a max height (for example 50% of the column) that scrolls inside its own frame, and never let it cut a card's last row. Give the transcript a minimum height (about 120 px) and fold the session facts (status, "No sandbox", Details) to one line while an approval waits. Mask the transcript's top edge with a fade instead of a hard cut. Let the composer's hint wrap inside its box, or drop it while the menu's own text says the same. Put the "changes wait" card in the flow, not over the transcript.

### NEW-18 minor: The Y and N key caps on the card type into the reply box right after Start

- **Area:** ui/src/agents (focus after `start`), ui/src/approvals (key handling and the Y/N hint)
- **Steps:** 1. Type a task and press Enter. 2. When the card appears, press N as its key cap shows. Then press Shift+N and type a message.
- **Expected:** N denies (the card says "Deny N", and the screen-reader line says "Y allows, N denies, Shift+N denies with a message"), or the card shows no key caps while typing would go elsewhere.
- **Actual:** Focus is in "Reply to Claude", so the reply box fills with "nNPlease only touch the README" and the request still waits (`newcomer-51-after-msg-deny.png`). Send is disabled while Claude works, so nothing goes out, but the text stays.
- **Why it matters for Nadia:** She does what the card tells her, and it does not work. Next time she will not trust the key caps.
- **Suggested fix:** After Start, put focus on the session (the heading) instead of the reply box, while a request waits. Alternatively, dim the key caps while a text field has focus and show "Esc, then Y" in their tooltip.

### NEW-19 minor: Seven signals for three reviews, and four "Review" pills for one task

- **Area:** ui/src/agents (AgentsPanel.svelte rows and `reviewHint`), ui/src/notify (Notifications.svelte), ui/src/shell (TitleBar badge)
- **Steps:** 1. Finish a Plan and split task with three workers. 2. Look at the screen.
- **Expected:** One clear place to act, for example a single "Review 3" on the task row. The workers show their state without repeating the action.
- **Actual:** A "Review" pill on the task row and on each worker row (4), the line "Changes wait for you: Review keeps or discards them.", the toast "Add notes for the project: 3 changes to review · Review", and a "3" badge on the title-bar inbox icon (`newcomer-35-split-done.png`). The toast stays on top of the Reviews dialog after she opens it (`newcomer-37-reviews.png`). The row titles are cut by the pills: "Task: Add notes for t…", "docs · Add notes f…", "lint · Add notes for…".
- **Why it matters for Nadia:** The owner's complaint was "too much text". Here the same instruction is shown seven times, and the part she reads first (the task name) is the part that gets cut.
- **Suggested fix:** Put the pill on the task row only ("Review 3"). Name the worker rows by the worker alone ("docs", "tests", "lint") under the task, without repeating the task's name. Drop the "Task:" prefix. Dismiss the review toast once the inbox opens. Show the hint line only until the first review is opened, as the other one-time hints do.

### NEW-20 minor: "No sandbox" is said four or five times on one screen

- **Area:** ui/src/agents (AgentsPanel.svelte `unsandboxed`, `sandbox`, `modeHint`), ui/src/approvals
- **Steps:** 1. On a machine without a sandbox (the headless run), start a task and look at the column while a card waits. 2. Go back to Tasks.
- **Expected:** The fact is stated once, where it decides something (the approval group, and the composer before Start), as the spec asks: always visible, but not repeated.
- **Actual:** "No sandbox" chip on the approval group. "No sandbox: agents can reach anything you can" under the Agents header. "No sandbox: the agent can read and change anything your user can." in every session view. "…without a sandbox." in the mode hint. These are four different wordings (`newcomer-31-split-approvals.png`, `newcomer-32-split-list.png`). All of them are yellow, the same colour as "Needs approval" and "Waiting for review".
- **Why it matters for Nadia:** Repeated warnings become wallpaper. She stops reading yellow text, and then misses "Needs approval".
- **Suggested fix:** Keep the chip on the approval group and one line under the Agents header, with a single wording ("No sandbox: Claude can change anything you can"). Drop the copies in the session view and the mode hint. Give "needs you" states and the sandbox warning different visual weights.

### NEW-21 minor: Approval cards repeat the object and show raw tool fields ("old string", "new string")

- **Area:** ui/src/approvals (Approvals.svelte card body)
- **Steps:** 1. Start "Fix the typo in the README". 2. Read the Run card, then the Edit card.
- **Expected:** One title and one easy-to-read body: "Run `ls -la`: List files". "Edit README.md" with the old and new text shown as a small diff (the previous round's card showed them side by side).
- **Actual:** The Run card has "Run `ls -la`", then a table with "command ls -la" and "description List files". The description is in mono, as if it were code. The Edit card has "Edit README.md", "In the project: README.md", then "file README.md", "old string teh" and "new string the". That is README.md three times, plus the tool's parameter names (`newcomer-45-after-deny.png`, `newcomer-31-split-approvals.png`). Three worker cards in a row repeat "Write NOTES.md / In the agent's folder: NOTES.md / file NOTES.md / content …".
- **Why it matters for Nadia:** "old string" is a programmer's field name, not her words. She has to read the card three times to find the one thing that differs.
- **Suggested fix:** Drop table rows that repeat the title (command, file). Show the description as plain text under the title. Render Edit as a two-line red and green diff, and Write as a short preview of the content. Keep the full raw input under Details.

### NEW-22 minor: Done tasks still offer Stop, and Interrupt is next to Stop with no explanation

- **Area:** ui/src/agents (AgentsPanel.svelte `.bar`)
- **Steps:** 1. Finish "Fix the typo in the README" and look at the bar. 2. While a card waits, look at the bar again.
- **Expected:** A Done task shows Close (and Reply). While working, one stop action, or two with clear words.
- **Actual:** "Stop" and "Close" stay next to a status of "Done" (`newcomer-24-after-allow-1.png`). While a card waits, the bar shows "Interrupt" and "Stop", and neither button has a tooltip. This is NEW-10 from the Quiet round, still open.
- **Why it matters for Nadia:** "Stop" on a finished task makes her think it is still running. She cannot tell Interrupt from Stop and will pick one at random.
- **Suggested fix:** Hide Stop when the session is idle after a finished turn. Rename Interrupt to "Pause turn" with the tooltip "Stops this step; you can reply", and give Stop the tooltip "Ends the session".

### NEW-23 minor: Transcript cards show plumbing: "Ovra · Worker docs asked for review · What Claude was told"

- **Area:** ui/src/agents (Transcript.svelte)
- **Steps:** 1. Run a Plan and split task to the end. 2. Open the task row.
- **Expected:** The coordinator's transcript tells her the outcome: "docs, tests and lint are done; 3 changes wait for review".
- **Actual:** Three grey cards, each "Ovra / Worker docs asked for review. / What Claude was told", where the last is a disclosure (`newcomer-36-task-open.png`). After a denial with a message, the denied step may also show a red "failed" (in the fake run its output was the message itself, so it was not recognised as a denial). The check relies on the CLI's English text "The user doesn't want to proceed…".
- **Why it matters for Nadia:** "What Claude was told" reads like something hidden from her. "failed" after her own Deny makes her think she broke something.
- **Suggested fix:** Merge the worker events into one line per worker under the "Started worker …" steps, with their status. Rename the disclosure "Message sent to the coordinator". Mark denied steps from the permission decision the core already records (the audit log has `allowed: false`), not from the output text.

### NEW-24 minor: Changing Claude's setup from the start screen still opens a repair form

- **Area:** ui/src/agents/setup (ClaudeSetup.svelte)
- **Steps:** 1. On the start screen, with "Claude is ready", click Change.
- **Expected:** A short list: the account in use, where usage is billed, "Use another account…", "Set path to claude…".
- **Actual:** A filled "Check again" (the brightest thing on the screen), "How to fix", a "Path to claude" field whose placeholder is cut ("Empty: search PATH and the usual install f…"), Cancel, Save path and "Use another Claude account" (`newcomer-41-claude-change.png`). The ready line shows no billing, while the workspace footer says "Claude · Ready".
- **Why it matters for Nadia:** She clicked Change out of curiosity and got a troubleshooting form. "How to fix" suggests something is wrong.
- **Suggested fix:** As in NEW-6 of the Quiet round: when ready, show "Details" instead of "How to fix", make Check again quiet, and put the path field behind its own link. Show the billing word ("uses your Claude plan") on the ready line.

### NEW-25 polish: Two shortcuts for finding a file, and the Ctrl+P she knows does nothing

- **Area:** ui/src/shell (TitleBar.svelte, the empty editor list), ui/src/keys
- **Steps:** 1. Open demo-app. 2. Compare the title bar with the empty editor. 3. Press Ctrl+P.
- **Actual:** The title bar says "Search or run Ctrl+K". The empty editor says "Find a file Alt+P". Ctrl+P does nothing in the browser tab, where the browser takes it. Typing "readme" also lists "Git: read blame of cursor line" and "Toggle screen reader mode in terminals" under Commands. The Git item has no group label column, while the other items do (`newcomer-09-search-readme.png`).
- **Suggested fix:** Use the same entry in the empty editor list ("Search or run Ctrl+K"). Show Commands only when they match a word start, or after a `>`. Give every command the same group column ("Git", "Terminal").

### NEW-26 polish: Icon-only Reviews and a folded Agents strip with an unlabelled robot

- **Area:** ui/src/shell (TitleBar.svelte, Shell.svelte strip)
- **Steps:** 1. Look at the title bar's right side. 2. Fold Agents.
- **Actual:** Reviews is an inbox tray icon. Its name appears only in the tooltip "Reviews Alt+Shift+R". The badge "3" sits on it after a split task. A folded Agents column is a 48 px strip with a robot icon, and when requests wait, also a "3 waiting" pill in the title bar (`newcomer-33-agents-hidden.png`). The "3 waiting" pill is clear. The icons are not, for someone who never hovered them.
- **Suggested fix:** Show the word "Reviews" next to the icon while something waits ("Reviews 3"). Give the strip's icon the label "Agents" under it, or use the same panel icon as the title-bar toggle.

### NEW-27 polish: Italic means two things: a preview tab and an ignored file

- **Area:** ui/src/editor (Tabs.svelte), ui/src/tree (FileTree.svelte)
- **Steps:** 1. Click README.md once. 2. Look at the tab and at `node_modules`, `target` and `build.log` in the tree.
- **Actual:** The preview tab's name is italic ("README.md" in italics, tooltip "README.md (preview)"). Ignored entries in the tree are also italic. A double click or the first edit keeps the tab, which works as in VS Code (`newcomer-12-tab-hover.png`). A pinned tab trades its × for a pin, so closing it takes two clicks (unpin, then ×), and nothing says so (`newcomer-15-pinned.png`).
- **Suggested fix:** Keep italics for the preview tab (VS Code users know it), and mark ignored tree entries with the dimmer colour alone. Give the pin button the tooltip "Unpin (then close)", or allow a middle click to close a pinned tab.

## Is there still too much text?

Visible words on screen at 1280x800 (the whole window):

| Screen                           | Quiet round | Round 8 |
| -------------------------------- | ----------- | ------- |
| Start, no projects               | 28          | 35      |
| Workspace, project open, no task | 156         | 109     |
| One approval waiting             | 195         | 180     |
| Three worker approvals           | 367         | 284     |
| Review inbox                     | 493         | 440     |

No text is below 12 px, and 12 px is used only for key caps, the footer "Claude · Ready" and badges. All the secondary text measured passes 4.5:1 in light mode: the Get started steps 6.68:1, the mode hint 6.68:1, the approval hint 5.89:1, "Needs approval" 5.90:1, the footer 5.89:1.

The start screen now has more words, and they are useful ones. The workspace with nothing running dropped by a third. Approvals and the inbox are still wordy, and most of it is repetition rather than new information: "No sandbox" (NEW-20), "Review" (NEW-19), the object repeated in each card (NEW-21), and the task name repeated in each worker row. Removing the repeats would bring the three-approval screen to about 180 words without losing a fact.

## What worked well

- The start screen explains Ovra in one sentence ("Open a folder, give Claude a task, then approve its steps and review its changes.") and shows the Claude status before she opens anything (`newcomer-01-start.png`). Path errors are specific and replace the hint. Opening took 239 ms.
- Get started is exactly what a newcomer needs: three steps, the next one highlighted, ticked off by doing them, and gone after the first approval (`newcomer-03-project-open.png`).
- The composer says what it is for ("Give Claude a task", "Describe a change, or ask about the code"). The mode menu explains both options in one line each, and the line under it changes with the mode (`newcomer-20-mode-menu.png`). The disabled Start explains itself ("Type a task first").
- Approval cards are safe. Allow once fills up while it arms, a one-time line explains Allow and Deny, cards for one task are grouped, and Deny with a message is one click away.
- Task rows put "needs you" first and show a clear "3 of 3 done" bar on the task row. Closing a task asks first and says the changes stay in Reviews (`newcomer-71-close-confirm.png`).
- File tabs work as in VS Code: a single click previews (italic), a second file replaces it, a double click or an edit keeps it. Edit and "Open in default app" are in the file's own bar.
- Search or run finds README.md first with highlighted letters, and its placeholder teaches the `>` prefix.
- The More menu is short and plain: Settings, Audit log, Agent browser, Keyboard shortcuts (`newcomer-06-more.png`).
