# Agent orchestrator (Andrii): Round 4 (2026-11-24)

Build: 81d16b1 on `main`, headless (fake-agent and fake-claude), Chromium 158.0.8127.36, 1280x800.
Scope: features since round 3: terminals, the Agents panel, Approvals, Claude onboarding and Trust
project, the review inbox (Alt+Shift+R in the browser), direct mode (baseline, attribution,
approve, reject). Also a re-check of ORCH-20, ORCH-21 and ORCH-22. Fixtures: `small` (`demo-app`),
`empty`. 13 scripted runs (r01-r13): the scripted fake agent, and the real Claude driver against
fake-claude playing `interrupted`, `denied_outside_folder`. Agent file writes were simulated by
writing files from the script while the recorded `ping` Bash call ran (the core counts such changes
as the agent's). A person's edits were made in Ovra's editor, in Ovra's terminal, or from the
script outside a tool call. Two runs started their own daemon to restart the core
(r10) and routed the page's WebSocket to drop the connection (r06, r09, r11).

## Summary

Andrii can run one agent per project, answer its requests from the keyboard, and approve or reject
its work in the inbox. Reject keeps edits that it knows came from outside the agent. The
connection problems from round 3 are fixed. The worst problem is that **Reject silently reverts
his own saves from Ovra's editor** when they land while the agent's shell command runs. Ovra knows
those saves came from him, the confirmation does not list what will be reverted, and there is no
undo (ORCH-28). He cannot run a second agent in a project from the UI at all (ORCH-29). The inbox
also misleads him: it is stale until the agent's next turn ends (ORCH-30), it forgets what he
decided while the session keeps changing files (ORCH-31), and its items vanish when the core
restarts (ORCH-32). It feels like a good single-agent loop. It is not yet the 5-10-agent
cockpit he wants, and in direct mode he cannot yet trust Reject with files he also touches.

## Re-check of round 3

| ID      | Round | Old severity | Status | Evidence (this build)                                                                                                                                                                                                                                                                                                                                                            |
| ------- | ----- | ------------ | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ORCH-20 | 3     | blocker      | fixed  | r09: I dropped the connection (socket closed, the HTTP probe failing as in a real core restart) and clicked the collapsed `docs` folder. `page.evaluate` answered 2 ms later. After the reconnect, `docs` was expanded with `guide.md`.                                                                                                                                          |
| ORCH-21 | 3     | major        | fixed  | r09: during the outage I created `src/while_offline.rs` and deleted `src/lib.rs`. After the reconnect, with no reload, the tree shows `while_offline.rs untracked` and `lib.rs deleted`. r06: a pending approval answered in a second window disappears from the first window's queue after its reconnect. The session status and the "Reviews (1 waiting)" count also catch up. |
| ORCH-22 | 3     | major        | fixed  | r09 a) I loaded the page while the core refused connections ("Core unavailable: could not connect, retrying…"), then let it accept. The Recent list showed `demo-app` with no error. b) With `token=nope`, a full-page "Ovra core was restarted … Open the new address … `OVRA_URL=`" message appears (`84-bad-token.png`), and retrying stops.                                  |

## Findings

### ORCH-28 blocker: Reject reverts the person's own saves made in Ovra while the agent's command runs, without listing them and without undo

- **Area:** crates/core (`orchestration/direct.rs`, attribution), ui/src/inbox (reject confirmation)
- **Steps (r08, r03):** 1. Open `demo-app`. Start a session (fixture `interrupted`) and allow its
  `ping -c 120 127.0.0.1` Bash call. 2. While it runs, open `Cargo.toml` in Ovra, press Edit, add
  `# my own note, typed in Ovra`, press Ctrl+S. 3. Interrupt the agent. 4. Open the inbox
  (Alt+Shift+R). Choose Reject…, then "Reject the changes".
- **Expected:** Ovra wrote `Cargo.toml` for the person through `fs.write_file`, so it should know
  that save was not the agent's. The file is "changed outside the agent" (or "both"), and Reject
  leaves it alone. At minimum, the confirmation lists each file that Reject will restore or
  delete, and the outcome offers an Undo.
- **Actual:** the inbox lists `Cargo.toml modified, +2 −0` with no marker, the same as the agent's
  files (`71-inbox.png`). The confirmation says only "Reject restores the agent's files from the
  start of the session." After Reject, `Cargo.toml` no longer has the note
  (`Cargo.toml has note: false`). The open `Cargo.toml` tab quietly shows the old text. Nothing says
  his save was undone. The same happens to a save from any other editor (r03: `README has PERSON
  edit: false`). The snapshot taken before the revert exists (`refs/ovra/snapshot` reflog has 5
  entries), but no part of the UI mentions it. Approve with "Commit the agent's files" would
  have committed the same note under the author `Claude <noreply@anthropic.com>` (r04 shows commits
  are authored that way).
- **Why it matters for Andrii:** direct mode is meant for watching an agent work in the same
  checkout and fixing small things along the way. Agent Bash calls (tests, builds) run for minutes,
  so "his save lands during the agent's command" is the normal case, not a corner case. Losing a
  saved edit to a button labelled "Reject…" that says nothing about his file breaks his trust in
  Reject.
- **Suggested fix:** in the core, record paths that clients write (`fs.write_file`, and later
  terminal commands started by the person) as the person's in the open interval, so they become
  `both` and are never reverted. In the inbox, list the files Reject will restore or delete in the
  confirmation step. After a reject, show "Undo reject" for that session (restore from the
  pre-reject snapshot) until the next agent turn. Add a core test with a client write during a
  running Bash interval.

### ORCH-29 major: only one agent per project can run from the UI, and the refusal does not say which agent blocks or what to do

- **Area:** ui/src/agents (`AgentsPanel.svelte`, `sessions.svelte.ts` always sends `mode: "direct"`), crates/core (`agents/requests.rs` message)
- **Steps (r01, r05):** 1. Start "Fix the typo in README". It asks for `ls -la`. 2. Type "Second
  task: write tests" and press Start. 3. Later, stop task A, start task B, then select A and press
  Resume with a message.
- **Expected:** a second task can run, isolated in its own worktree (ADR 0005: "the caller may start
  it isolated instead"), or at least the refusal names the agent that holds the checkout. It
  offers "Stop it" or "Start isolated instead" and keeps the typed task.
- **Actual:** a red line "another agent already works in this project's main checkout" appears under
  the new-task field (`04-second.png`). It does not say which session or offer a choice. A session
  that is idle ("Waiting for you") keeps the lock indefinitely, so he must Stop it, which ends
  replies. Resume of A while B is idle fails with the same message. The message shows under the
  _new task_ field, about 300 px above the Resume button he pressed (`42-resume.png`). The core's
  other message with the agent id ("another agent (s1) …") never reaches the UI.
- **Why it matters for Andrii:** he runs 5-10 agents. Today he gets one per project, and finding out
  why takes a failed Start. The coordinator and worker UI is planned for phase 3 and is not a finding by itself.
  The problem is that today's flow gives no isolated choice and no way to see who holds the
  checkout.
- **Suggested fix:** until the coordinator UI lands, add "Start isolated (own worktree)" next to
  Start when a direct writer exists, or make that the default for the second session. Mark the
  direct writer in the session list ("works in your checkout"). Show refusals next to the control
  that caused them, and name the blocking session with a "Show" link.

### ORCH-30 major: the inbox shows a stale file list until the agent's next turn ends

- **Area:** crates/core (`orchestration/mod.rs` emits `review.changed` only on turn end and status changes), ui/src/inbox (`reviews.svelte.ts`, opening the dialog does not reload)
- **Steps (r04, r03):** 1. In a direct session, the agent's Bash call creates `src/agent.rs` and
  rewrites `docs/guide.md`. Then interrupt it. 2. The person edits `src/agent.rs` and `src/lib.rs`
  (`fs.changed` arrives within 50 ms). 3. Open the inbox. 4. Send the agent a comment so that a
  turn ends.
- **Expected:** the inbox shows the current diff when it opens and as files change: `src/agent.rs`
  "Also changed outside the agent", `src/lib.rs` "Changed outside the agent".
- **Actual:** at step 3 it lists "2 files": `docs/guide.md`, and `src/agent.rs added, +1 −0` with
  no marker. `src/lib.rs` is missing. Only after the agent's next turn does it read "3 files, 2
  changed outside the agent" with both markers (`30-inbox-before-turn.png` vs
  `31-inbox-after-turn.png`). An idle or stopped agent never ends another turn, so the list stays
  stale. Reject still re-checks on the core, so the stale list did not cost data here. But the
  outcome "Left for you: src/agent.rs" surprised me, because the list had said the file was purely the agent's.
- **Why it matters for Andrii:** he decides from the file list and its markers. A list that leaves
  out his own changes and hides "both" files makes Approve and Reject a guess.
- **Suggested fix:** emit `review.changed` for open direct items when `fs.changed` touches the
  project (debounced), and reload the list whenever the inbox opens.

### ORCH-31 major: after a decision the item forgets its files, while the still-running session's later changes pile into it unreviewed

- **Area:** crates/core (`orchestration/review.rs`), ui/src/inbox, ui/src/agents
- **Steps (r05, r04):** 1. Task A finishes. Approve its item. 2. Reply to A ("continue"). It runs
  another tool, and a file `src/a2.rs` appears. 3. Stop A and start task B, which changes
  `src/b1.rs`.
- **Expected:** a decided item keeps a record of what was approved or rejected, with the files and
  the diff. New changes from a session that continues after a decision open a new review, or reopen
  the item, and count in "Reviews (N waiting)". The agent is told when its work was rejected.
- **Actual:** right after Approve or Reject the item reads "0 files" and "This review has no files"
  (`failure.png` in r05, `33-rejected.png`), so there is no record of what he decided on. The session
  keeps running. Its later changes, and B's changes, show up under A's _Approved_ item ("2 files, 2
  changed outside the agent") with no Approve or Reject, and the counter says "Reviews" with
  nothing waiting. After Reject, the transcript has no line about the revert. The agent's next
  turn assumes its changes are still there (r04-reject: the session is "Waiting for you", with no
  note).
- **Why it matters for Andrii:** he approves early and lets agents continue. Work done after the
  first decision escapes review, and he cannot see what he already approved.
- **Suggested fix:** freeze the file list and diffs at decision time (show them read-only). When the
  same session changes files again, reopen the item as a new round or create a new one. Keep other
  sessions' changes out of a decided item. On Reject, add a transcript entry and tell the agent
  (the spec already does this for a comment after a reject).

### ORCH-32 major: open reviews disappear when the core restarts, and direct-mode changes can no longer be rejected

- **Area:** crates/core (`orchestration` state is in memory), crates/store
- **Steps (r10):** 1. Own daemon with a data folder. Start a session, let it change files, and leave
  it idle with "Reviews (1 waiting)". It then asks for `git status`. 2. Kill the daemon and start it
  again with the same data folder. 3. Open the new URL and the project. Open the inbox.
- **Expected:** the open review survives (the spec: "after a restart the item's files all count as
  changed outside"). Reject can at least list the files, and the session says why it stopped.
- **Actual:** the session is kept ("Persisted task, Stopped", transcript intact, Resume offered).
  The inbox says "No agent changes wait for review." and the button reads "Reviews". The
  pending `git status` request is gone without a word: the transcript ends at "Tool Bash Run git
  status".
- **Why it matters for Andrii:** he restarts the daemon often. After a restart, the agent's
  uncommitted changes look like his own working-tree changes, with no review and no way to reject
  them.
- **Suggested fix:** persist review items with their baseline (the snapshot is already a git
  object) next to sessions in `SessionStore`. After a restart, list them with every file marked
  "changed outside" as the spec says. Mark sessions ended by a restart ("Stopped when Ovra
  restarted; 1 request was dropped").

### ORCH-33 minor: the person's own edits open a "waiting" review item

- **Area:** crates/core (`orchestration/direct.rs`), ui/src/inbox
- **Steps (r02, r11):** 1. Start a session, allow `ls -la` and the README edit (the scripted agent
  writes nothing). 2. Append to `README.md` and create `src/person.rs` from outside, or run
  `echo '// fmt' >> src/main.rs` in Ovra's terminal.
- **Expected:** with no agent changes, nothing waits for review, or the item says "Only changes made
  outside the agent".
- **Actual:** "Reviews (1 waiting)" and an item "2 files, 2 changed outside the agent" with Approve
  and Reject (`11-inbox.png`). Reject then reports "Rejected." and "0 files". The confirmation had said changes
  made outside "are listed for you", but they are not.
- **Why it matters for Andrii:** the count is his to-do list. Items that ask him to review his own
  edits are noise across 5-10 agents.
- **Suggested fix:** count an item as waiting only when at least one file is `agent` or `both`. Keep
  the outside-only files visible in a muted section, and list them in the outcome as promised.

### ORCH-34 minor: approvals do not say which project they are for, and there is no overview of all running agents

- **Area:** ui/src/approvals, ui/src/notify, ui/src/agents
- **Steps (r07, r06):** 1. Start a session in `demo-app` and another in `empty`. 2. Stay in `empty`.
  3. Reload the page, or open a second window without a project.
- **Expected:** every card and notification names the project ("demo-app: Small: refactor wants to
  Edit README.md"). A compact list shows every running session across projects.
- **Actual:** the cards read "Small: refactor asks to use Edit / Edit README.md" and "Empty:
  scaffold asks to use Bash", with no project (`62-after.png`). Pressing Y allowed an edit of
  `README.md` in the other project. After a reload, or in a window without that project open, the
  card reads "Session s1 asks to use Bash" (r06). The Agents panel says "Open a project to start an
  agent" while that agent waits. Only the open project's sessions are listed.
- **Why it matters for Andrii:** he approves from whichever project is in front of him. "Edit
  README.md" means something different in every repository.
- **Suggested fix:** put the project name on every card and notification. Load session titles for
  requests of projects that are not open (`agents.list` without a project filter). Add a header
  indicator, "3 agents running, 1 needs you", that lists sessions across projects.

### ORCH-35 minor: after Approve or Reject, focus falls to the page body and Escape no longer closes the inbox

- **Area:** ui/src/inbox (`Inbox.svelte`)
- **Steps (r05):** 1. Open the inbox. 2. Press Approve (or confirm Reject). 3. Press Escape.
- **Expected:** focus moves to the outcome line or the next waiting item. Escape closes the dialog.
- **Actual:** the pressed button is removed, `document.activeElement` is `<body>`, and Escape does
  nothing: the dialog stays open. My script then hung on a click behind the backdrop. Focus is
  outside an `aria-modal` dialog.
- **Why it matters for Andrii:** he clears a queue of reviews from the keyboard. Each decision now
  needs a mouse click on Close.
- **Suggested fix:** after a decision, focus the next open item (or the outcome text with
  `tabindex=-1`). Handle Escape at the backdrop or window level while the dialog is open.

### ORCH-36 minor: "Approve and commit" is disabled with an empty message and no reason

- **Area:** ui/src/inbox (`describe.ts` `defaultCommitMessage`)
- **Steps (r04):** 1. Open the review of a session that did not call `request_review` (`summary:
  null`). 2. Tick "Commit the agent's files".
- **Expected:** a drafted message, such as the task title "Run the shell command …", or a hint
  "Write a commit message".
- **Actual:** the message field is empty and the button has `aria-disabled="true"`. Nothing says
  why (`32-commit-empty.png`).
- **Suggested fix:** fall back to the session title, and show a hint next to the disabled button.

### ORCH-37 minor: a reload loses the terminal, while its shell keeps running out of reach

- **Area:** ui/src/terminal, crates/pty, protocol `terminals.*` (no list or reattach)
- **Steps (r11, r12):** 1. Open a terminal (Ctrl+`). Run `echo … >> src/main.rs`, then `exec sleep 777`.
  2. Reload and reopen the project. Press Ctrl+`.
- **Expected:** the same terminal with its scrollback, or a clear "terminal ended" message and the
  process stopped.
- **Actual:** "Terminal 1" is a new shell with no history (`A2-terminal-after-reload.png`). The old
  `sleep 777` keeps running (`pgrep` count 2 before and after the reload), and no part of the UI reaches it.
- **Why it matters for Andrii:** he keeps watchers and test loops in terminals next to his agents.
  After a reload they are invisible but still change files, and in direct mode those changes show
  up as "changed outside the agent".
- **Suggested fix:** add `terminals.list` and reattach with a scrollback buffer, or close a client's
  terminals when the client goes away and say so.

### ORCH-38 polish: the session list does not show review state, and the setup cards take a quarter of the panel

- **Area:** ui/src/agents
- **Steps (r05, r07):** run two sessions, one with an open review, and look at the right column at 1280x800.
- **Actual:** the list shows "Waiting for you" (agent idle) but not "Waiting for review", and the two
  phrases are easy to confuse. "Claude is ready" and "Project settings: not loaded" stay expanded
  (about 180 px) when nothing needs doing. With two approval cards the session list and transcript
  fall below the fold (`62-after.png`). The task notification "Task B finished and waits for you"
  has no "Open review" action.
- **Suggested fix:** add a review badge per session that opens the inbox on that item. Collapse the
  setup cards to one line once Claude is ready and the trust choice is made. Add "Review" to the
  finished notification.

## What worked well

- Approvals are quick and keyboard-first. The card appeared 153 ms after Start. Alt+Shift+A
  focuses the oldest request. Y, N and Shift+N work, and focus moves to the next card. The
  notification's Review button focuses the right card even for another project.
- "Why it asks: Ovra asks before an agent changes files outside its folder. (hook)" plus the
  agent's note tells him exactly why a request exists (r13). "Deny with message" reaches the agent
  word for word, and it complies.
- Reject keeps what it knows is not the agent's: a file changed by both is left as is and
  reported as "Left for you: src/agent.rs" (r04). Untracked agent files are deleted, and modified
  ones are restored exactly. Stop during a running tool still attributes that tool's files to the
  agent (r12).
- The "Changed outside the agent" and "Also changed outside the agent" markers are clear and read
  out as text.
- Commits made on approve are authored as `Claude <noreply@anthropic.com>` and contain only the
  agent's files, so his history shows which commits came from agents (r04).
- Sessions and transcripts survive reloads and core restarts, and Resume is offered for stopped
  sessions (r10). After a dropped connection, approvals, session status and the review count
  resync without a reload (r06).
- Interrupt explains itself in the transcript ("this call was stopped while running … its changes
  may have been written"), which is exactly what he needs before deciding on a review.
- Trust project explains what loads and what does not, and that Ovra asks again if settings appear
  after a pull (r13).
