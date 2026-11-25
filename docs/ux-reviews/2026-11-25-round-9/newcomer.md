# Nadia, the newcomer: round 9 (2026-11-25)

Build: dd1b0d4 (main), headless with the scripted fake agent, Playwright Chromium 158.0.8127.36, 1280x800 (also 1024x700), light.
Scope: what changed since round 8: the Settings view (rail gear and Ctrl+,) with its Trust, Rules, Network, Servers, Claude, Notifications, Keyboard, Accessibility and About panes; Reviews and the Audit log as main-area views, Keep changes from there; the servers dock; "Set up git" in a folder that is not a git repository; the new list rows in the tree and the Git panel (hover actions, status dots); the diff bar; approval cards with "N more requests". Plus a re-check of round 8. Fixtures: `small` (as demo-app, seeded), `empty`, and a plain folder `my-site` (two files, no git) typed in by hand.

Runs: eleven scripts. Screenshots are in `/mnt/project-files/ovra/screenshots/round-9/` as `newcomer-*.png`. Words were counted over every visible text node in the window, as in round 8.

Not checked: the Edit approval card (the run allowed it without a screenshot), a real dev server (the fake server exits with "no such preset"), real keychain secrets.

## Summary

Nadia could do every task in scope without help. Settings reads well (one line per pane, the project's panes named "This project · demo-app"), Set up git shows up exactly where Plan and split needs it and works in one click, and the approval cards and the Done state are much calmer than in round 8. The worst problems are about the main area: a view hides all her open tabs and the obvious way back (the highlighted Files icon) does nothing (NEW-28), and Reviews, squeezed between the tree and the Agents column, keeps the diff below the fold at 1024x700 while a toast sits on top of it (NEW-29). Smaller issues: "2 more requests" dims the window and buries the task header under the cards (NEW-32), Escape in a Settings field throws away what she typed (NEW-34), and the Network and Servers panes talk about a sandbox this machine does not have (NEW-33).

New: 0 blocker, 2 major, 8 minor, 3 polish.

Re-check of round 8 (`2026-11-25-round-8/newcomer.md`):

| ID     | State            | Note                                                                                                                                                                                                                                                                                                                                                            |
| ------ | ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NEW-16 | **fixed**        | With Agents folded, the empty editor's "New task" unfolds the column and puts the cursor in "Describe a change, or ask about the code" (`newcomer-111-newtask-unfolded.png`).                                                                                                                                                                                   |
| NEW-17 | **partly fixed** | The approvals block now has its own height and scrolls, the composer hint wraps, and "3 changes wait for your review. / Review" is in the flow. Still: after "2 more requests" the task header is covered by the cards, and "Started worker lint" is cut through its letters by the session facts above it (`newcomer-120-after-escape-scrim.png`). See NEW-32. |
| NEW-18 | **open**         | After Start, focus is in the reply box ("Claude is working…"). Pressing N, as the card's key cap says, types "n" into it; the request still waits (`newcomer-104-after-N.png`).                                                                                                                                                                                 |
| NEW-19 | **partly fixed** | One "Review" button on the task, worker rows no longer repeat it, and the coordinator says "Workers docs, tests and lint asked for review" once. The toast "Add notes for the project: 3 changes to review · Review" still stays on top of the Reviews view, and counts down there (`newcomer-46-reviews.png`, `newcomer-48-after-keep.png`).                   |
| NEW-20 | **mostly fixed** | Two places now: the "No sandbox" chip on the approval group and "No sandbox: Claude can change anything you can" in the session. One wording. The mode hint no longer repeats it.                                                                                                                                                                               |
| NEW-21 | **fixed**        | "Run `ls -la`" / "List files" in plain text; a Write card shows "In the agent's folder" and the content preview. No raw field names. (Edit card not re-checked.)                                                                                                                                                                                                |
| NEW-22 | **fixed**        | A Done task shows only Close. While working, Interrupt has the tooltip "Stops this step; you can reply" and Stop "Ends the session". The word "Interrupt" stays; fine with the tooltip.                                                                                                                                                                         |
| NEW-23 | **partly fixed** | Worker events are one folded line. After a Keep it grows to "Worker docs asked for review · Worker tests asked for review · Worker lint asked for review · Worker lint had its work approved by the person" (NEW-39).                                                                                                                                           |
| NEW-24 | **fixed**        | Start screen and Settings > Claude show "Claude is ready", "Program `claude` found automatically", and two quiet buttons, Check again and Set path… (`newcomer-09-settings-claude.png`). Billing is still not shown (NEW-38).                                                                                                                                   |
| NEW-25 | **open**         | The empty editor still says "Find a file Alt+P" while the title bar says "Search or run Ctrl+K"; Ctrl+P still does nothing in the browser.                                                                                                                                                                                                                      |
| NEW-26 | **open**         | The folded Agents strip is still an unlabelled robot icon; Reviews is still an icon with a badge (`newcomer-110-agents-folded.png`).                                                                                                                                                                                                                            |
| NEW-27 | **open**         | Ignored entries (`node_modules`, `target`, `build.log`) are still italic, like a preview tab.                                                                                                                                                                                                                                                                   |
| NEW-14 | **open**         | `~/my-site` still gets "Use the full path to a folder, for example /home/you/my-app." A trailing slash works.                                                                                                                                                                                                                                                   |
| NEW-4  | **open**         | "Also changed in another open review" is still on every file of the three-worker task (`newcomer-46-reviews.png`).                                                                                                                                                                                                                                              |

## Findings

### NEW-28 major: A main-area view hides every open tab, and the highlighted Files icon does not bring them back

- **Area:** ui/src/shell (Shell.svelte main area, Rail.svelte, ViewFrame.svelte, main-view.svelte.ts)
- **Steps:** 1. Open README.md and Cargo.toml (two pinned tabs). 2. Click the gear in the rail (or open Reviews, or More > Audit log). 3. Look for her files. 4. Click the Files icon in the rail, which is still drawn as selected.
- **Expected:** She can see that her files are still open and how to get back: the tab strip stays with the view shown as a tab ("Settings ×"), or the view's header says "Back to Cargo.toml". The selected rail icon brings back what it shows.
- **Actual:** The tab strip disappears completely while the view is open (log: `settings open, tabs visible: none`). The only way back on screen is a bare × in the top-right corner (tooltip "Close (Escape)"). Clicking the Files icon does nothing (`after rail Files: view=1`, `newcomer-100-after-rail-files.png`), and at that moment the rail shows two icons as selected, Files and Settings (`newcomer-04-settings.png`). What does work: Escape, the ×, the gear again, a click on a file in the tree, and opening a file from Search or run; all bring both tabs back with focus where it was (`newcomer-50-after-escape.png`).
- **Why it matters for Nadia:** She reads "my tabs are gone" as "Ovra closed my files". She does not know Escape closes a page, and the one icon that looks like "files" does nothing.
- **Suggested fix:** Keep the tab strip visible and show the open view as one more tab (with its own ×), or put a text button "Back to Cargo.toml" in the view header next to the title. Make a click on the rail's Files, Search or Git icon close the main-area view as well as switch the sidebar. Do not draw Files as selected while a view covers the editor (only the gear).

### NEW-29 major: Reviews in the main area leaves no room for the diff: below the fold at 1024x700, and covered by the toast

- **Area:** ui/src/inbox (Inbox.svelte layout), ui/src/notify (review toast), ui/src/shell (main-area width)
- **Steps:** 1. Run "Add notes for the project" with Plan and split, allow the three writes. 2. Click Review. 3. Do the same at 1024x700.
- **Expected:** The change she is asked to keep is in view next to Keep changes, as the commit message promises ("the diff beside the list").
- **Actual:**
  - At 1280x800 the main area is 632 px wide. The list and the detail sit side by side, but the diff starts under the summary, the "Commit the agent's files" checkbox, Keep, Reject and a comment box: one diff line is visible at y=624, and the toast covers the rest (`newcomer-46-reviews.png`).
  - At 1024x700 the main area is 376 px. The list is stacked over the detail and clipped at 175 px (content 214 px), so the third worker shows only "docs" with no status. The diff starts at y=705, below the window (log: `diff {"y":705,…}`). Keep changes is on screen; the change is not (`newcomer-121-reviews-1024.png`).
  - The toast "Add notes for the project: 3 changes to review · Review" stays over the view and updates to "2 changes" after each Keep (`newcomer-48-after-keep.png`).
- **Why it matters for Nadia:** "You approve the results" is the promise she installed Ovra for. Here she can press Keep without having seen what she keeps, and the screen gives her no hint that the diff is further down.
- **Suggested fix:** Put the diff right after the one-line summary and move "Comment to the agent" below the diff. When the main area is narrower than about 700 px, fold the Agents column while Reviews is open (and unfold it on close), or turn the item list into a compact "lint · 1 of 3 ▾" selector. Dismiss the review toast while Reviews is open (NEW-19).

### NEW-30 minor: Keep changes stays on the kept item and says "Kept" three times

- **Area:** ui/src/inbox (Inbox.svelte after `keep`)
- **Steps:** 1. In Reviews, on "lint", click Keep changes.
- **Expected:** "Kept lint. Undo" once, and the next waiting review (tests) opens.
- **Actual:** The view stays on lint: "Kept" next to the title, "Kept." as the body, "Kept · 1 file" in the list, plus a live-region "Kept." (`newcomer-48-after-keep.png`). She has to find and click "tests" herself. The kept diff stays on screen as if it were still waiting.
- **Why it matters for Nadia:** After three Allow clicks she expects the same rhythm here. Staying on a finished item makes her wonder whether the click worked.
- **Suggested fix:** Move to the next waiting item after Keep or Reject, with one line "Kept lint · Undo" at the top. Show the kept state once (the status word in the list).

### NEW-31 minor: Review toast stays on top of the Reviews view

Folded into NEW-29 and NEW-19 above; listed for the coordinator's area grouping.

- **Area:** ui/src/notify (Notifications.svelte)
- **Steps:** 1. Finish a Plan and split task. 2. Click Review in the toast or the task.
- **Expected:** The toast goes away once she is where it points.
- **Actual:** It stays, covering the bottom of the diff and Send comment, and updates its count after each Keep.
- **Suggested fix:** Dismiss review toasts for a project while its Reviews view is open, and do not show new ones there.

### NEW-32 minor: "2 more requests" dims the whole window and scrolls the cards over the task header

- **Area:** ui/src/approvals (Approvals.svelte `showMore`, the scrim, the sticky group header)
- **Steps:** 1. Start "Add notes for the project" with Plan and split. 2. Click "2 more requests" under the first card.
- **Expected:** The other two cards appear under the first, the group header "Task: Add notes for the project · No sandbox" stays readable, and nothing else changes.
- **Actual:** The whole window except the approvals turns grey (the focus scrim), with no word about why. The list scrolls so the first card's title is gone and its bottom half covers the group header: at the header's centre the topmost element is the card's "In the agent's folder" (`newcomer-43-after-more.png`, `newcomer-62-after-more-mouse-away.png`). The third card (lint) is below the block's edge. Under it, "Started worker lint" is cut through the letters by the session facts box (`newcomer-120-after-escape-scrim.png`).
- **Why it matters for Nadia:** A grey window looks like a modal she must close. Text cut through the middle looks broken, and the header she needs ("which task is this?") is the part hidden.
- **Suggested fix:** Give the group header a z-index above the cards (it is sticky but drawn under them). After "more", scroll so the new card sits fully under the header, not centred. Show the scrim only when a card got focus from the keyboard (Go to pending approvals, Tab), not after a mouse click. Mask the transcript's top edge with a fade instead of a hard cut.

### NEW-33 minor: Network and Servers panes promise a sandbox this computer does not have

- **Area:** ui/src/settings (Settings.svelte pane lines), ui/src/network, ui/src/servers (ServersPanel.svelte)
- **Steps:** 1. On a machine where the session says "No sandbox: Claude can change anything you can", open Settings > Network, then Servers.
- **Expected:** The panes say plainly whether their rules apply here.
- **Actual:** Network: "What sandboxed agents may reach…", "Only these, through Ovra's proxy. Everything else is refused and logged." Servers: "Dev servers run in the sandbox, where agents can open your app." and "Runs as typed, without a shell, in the sandbox". Nothing says that agents here are not sandboxed, so the host list does not limit them (`newcomer-07-settings-network.png`, `newcomer-08-settings-servers.png`). Network also opens with an orange "Secrets can't be saved: the system keychain is not reachable." before she ever wanted a secret.
- **Why it matters for Nadia:** She reads "Everything else is refused" literally and believes Claude cannot reach the internet, while the session view says the opposite.
- **Suggested fix:** When the platform has no sandbox, start both panes with one calm line: "No sandbox on this computer: agents are not limited to these hosts yet." Show the keychain notice only when she starts filling the Secrets form.

### NEW-34 minor: Escape in a Settings field closes Settings and throws away what she typed

- **Area:** ui/src/shell (ViewFrame.svelte `onkeydown`), ui/src/policy (Rules form), ui/src/servers (Add server form)
- **Steps:** 1. Settings > Rules. 2. Type `npm test` into "Match (optional)". 3. Press Escape (to close the browser's autocomplete, or out of habit). 4. Press Ctrl+, to reopen.
- **Expected:** Escape leaves the field (or clears it first); a second Escape closes Settings. A draft comes back when Settings reopens.
- **Actual:** The first Escape closes Settings (`after esc in input: settings open=0`), and on reopen the field is empty (`reopen match field=`).
- **Why it matters for Nadia:** She loses work from one key she presses all the time in VS Code.
- **Suggested fix:** In ViewFrame, ignore Escape when it comes from a text field with a value (blur it instead), or keep form drafts per pane while the project is open.

### NEW-35 minor: After the first dev server, the Servers pane offers only "Edit .ovra/servers.json", and a failed start says "exited (code 1)"

- **Area:** ui/src/servers (ServersPanel.svelte, ServersDock.svelte)
- **Steps:** 1. Settings > Servers, add "web", `pnpm dev`, port 5173. 2. Look for a way to add a second server or fix the first. 3. Click Start.
- **Expected:** An "Add server" button stays under the list, each server has Edit and Remove, and a failed start says what happened.
- **Actual:** The form disappears once one server exists; the pane shows "web pnpm dev port 5173 Start" and the link "Edit .ovra/servers.json" (`newcomer-94-server-started.png`). Start opens the dock with "web · exited (code 1) · port 5173 · in project folder · started by you · run 1", a disabled Stop, and the reason ("error: no such preset") only behind Output (`newcomer-94b-server-output.png`). The dock's empty state ("Start one from its definition; agents can start them too.") uses "definition", a word she has not seen anywhere else.
- **Why it matters for Nadia:** Editing JSON by hand is what she hoped settings would save her from. "code 1" and "run 1" are the codes that scare her.
- **Suggested fix:** Keep the Add form behind an "Add server" button; give each row Edit and Remove. On a failed start show "Stopped: the command failed" and open Output automatically. Write the dock's empty line as "Start a server from Settings > Servers; agents can start them too."

### NEW-36 minor: Set up git works, but its buttons look like text and the next step is not said

- **Area:** ui/src/git (GitPanel.svelte empty state, CommitBox.svelte), ui/src/agents (AgentsPanel.svelte `agent-needs-git`), crates/fs-index (tree status in a repository without commits)
- **Steps:** 1. Open `/tmp/…/my-site/` (no git). 2. Open the Git panel. 3. In the composer pick Plan and split and type a task. 4. Click Set up git in the composer. 5. Look at the Git panel and the tree.
- **Expected:** A clear button, then a hint about the first commit, and the tree marks the two new files as the Git panel does.
- **Actual:** "Not a git repository / Git keeps a history, so you can review and undo changes." is clear, but "Set up git" under it is plain text without a button shape (`newcomer-82-git-norepo.png`). In the composer it wraps into "Set / up git" next to "Needs git: Plan and split works on copies of a git repository." (`newcomer-83-split-needs-git.png`). After the click the Start button wakes up and Plan and split works, keep included (`newcomer-87-keep-fresh-repo.png`); the Git panel shows "master", "No commits yet.", "Stage files first." and two untracked files (`newcomer-84-after-setup.png`). The tree shows no dots for those files, even after a reload (`newcomer-86-tree-after-setup.png`).
- **Why it matters for Nadia:** She has never run `git init`. One success line and one next step would teach her what just happened; the silent tree makes the two panels disagree.
- **Suggested fix:** Draw both "Set up git" actions as buttons and keep the composer's on one line. After set-up, say "Git is set up. Commit your files to keep a first version." with a "Commit all" button. Have the tree show untracked status in a repository with no commits yet.

### NEW-37 minor: Git status is a 6 px coloured dot with its meaning only in the dot's own tooltip

- **Area:** ui/src/tree (FileTree.svelte), ui/src/git (FileSection.svelte)
- **Steps:** 1. Expand `src`. 2. Try to tell `main.rs` (modified) from `new_module.rs` (untracked). 3. Hover the row.
- **Expected:** As in VS Code, a letter (M, U) or a coloured name, and the status in the row's tooltip.
- **Actual:** Both rows have a 6x6 dot, brown `rgb(127,83,0)` or green `rgb(27,108,71)`, and nothing else (`newcomer-71-tree-hover-modified.png`). The row tooltip is just the path (`src/lib.rs`); the words "M: modified" are only on the 6 px dot. The tree rows have no hover actions; the Git panel rows show "+" (Stage) and an open icon on hover (`newcomer-74-git-row-hover.png`), and their focus ring stops before the dot and the icons (`newcomer-77-diff-after-escape.png`).
- **Why it matters for Nadia:** Colour alone does not tell her which files Claude added and which it changed, and she will not find a 6 px tooltip.
- **Suggested fix:** Add the status to the row tooltip ("src/main.rs · modified"), show the letter next to the dot on hover and focus, and draw the focus ring around the whole row including its actions.

### NEW-38 polish: Settings > Claude promises "who pays for them" and then says nothing about billing

- **Area:** ui/src/settings (ClaudePane.svelte, panes.svelte.ts line)
- **Steps:** 1. Settings > Claude.
- **Actual:** The line reads "The Claude Code on this computer that runs your tasks, and who pays for them." The pane shows "Claude is ready" and the program, but no Billing row (it appears only when `billingText` returns one) (`newcomer-09-settings-claude.png`).
- **Suggested fix:** When billing is unknown, show "Billing: your Claude sign-in (not reported)"; or drop "and who pays for them" from the line.

### NEW-39 polish: "Worker lint had its work approved by the person"

- **Area:** ui/src/agents (Transcript.svelte, the folded Ovra notes)
- **Steps:** 1. Keep one worker's changes. 2. Read the coordinator's transcript.
- **Actual:** "Worker docs asked for review · Worker tests asked for review · Worker lint asked for review · Worker lint had its work approved by the person" (`newcomer-50-after-escape.png`).
- **Suggested fix:** "docs, tests and lint asked for review · You kept lint". Say "you", not "the person".

### NEW-40 polish: Small words in Reviews and the rail

- **Area:** ui/src/inbox, ui/src/shell (Rail.svelte)
- **Actual:** The checkbox "Commit the agent's files" under Keep changes, with no hint whether it commits on her branch. "Agent's summary:" prefixes a one-line summary. The rail draws Files and Settings as selected together.
- **Suggested fix:** "Also commit these files (on main)" with the branch named; drop "Agent's summary:"; one selected rail icon at a time (see NEW-28).

## Is it clear how to get back from a main-area view to the editor?

Partly. Escape, the × and a click on any file all work, and focus returns to where she was. But nothing on screen says her tabs are still open, the × has no word, and the most likely click (the highlighted Files icon) does nothing (NEW-28). The diff opened from the Git panel is different again: it is a tab ("main.rs diff") with its own ×, which she understood at once, and Escape closes it.

## Is anything still too wordy?

Visible words in the whole window at 1280x800:

| Screen                             | Round 8 | Round 9    |
| ---------------------------------- | ------- | ---------- |
| Start, one recent project          | 35      | 37         |
| Workspace, project open, no task   | 109     | 114        |
| One approval waiting               | 180     | 158        |
| Three worker approvals             | 284     | 220        |
| Reviews (now with tree and Agents) | 440     | 377        |
| Settings panes (whole window)      | –       | 126 to 235 |

Approvals lost the repetition (NEW-20, NEW-21). Settings panes are short, but Network (235) and Servers (199) each carry three explanatory sentences that could be one; for example Secrets has "The proxy adds a token for one host; only sandboxed agents get it, and never see it. They can still do anything it allows there: keep its scope narrow." plus "Kept in the keychain, never shown again. The same name replaces it." plus "Saving needs the system keychain." Rules says "Deny refuses, Ask asks you first, Auto allows. Anything else is asked." and then "No rules yet: what your Claude settings do not allow is asked about." which says the same thing twice. Reviews still spends a sentence on what Keep does ("Keep changes copies the worker's changes into your project folder.") above the button named Keep changes.

## What worked well

- Settings is easy to scan: two groups, "This project · demo-app" and "Ovra", a noun per pane and one line under each title. Search or run finds "Settings Servers" and "Show servers" for the word "server" (`newcomer-04-settings.png`).
- Set up git appears exactly where Plan and split needs it, says why in one sentence, and after one click the same task starts and its changes can be kept, even in a repository without commits.
- "New task" now unfolds the Agents column (NEW-16), and a Done task shows only Close (NEW-22).
- Approval cards are plain: "Run `ls -la`" / "List files", "Write NOTES.md" with a content preview, the task name and one "No sandbox" chip.
- The diff bar is quiet: path, "Unstaged changes", Open file, and "Stage hunk" only on hover (`newcomer-76-diff-hunk-hover.png`).
- The Audit log reads like sentences ("Server web started · Run 1, started by a person.") with a single filter and "Refused only" (`newcomer-98-audit.png`).
- Escape out of any main-area view brings back the exact tabs and focus.
