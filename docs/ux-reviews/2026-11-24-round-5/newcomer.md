# Nadia, the newcomer: Round 5 (2026-11-24)

Build: 175ef39 (main), headless, scripted fake agent (`--fake-agent`), Playwright's bundled Chromium 158.0.8127.36 (Linux), 1280x800 (one check at 1600x1000).
Scope: phase 3 tasks 3.C and 3.E: "Start task" next to "Start" in the Agents panel, the workers' approvals (Write NOTES.md) in the Approvals panel, and their items in the review inbox (Alt+Shift+R, or the "Reviews" button). Fixtures: `small` (seeded), `empty` (a git repository without commits), and a plain folder that is not a git repository, created by the script (`my-site-*`, with `index.html` and `README.md`).

Runs `s1` to `s8`. Screenshots and logs are in `/tmp/claude-0/ux-newcomer/<run>/` and are named here as `<run>/<file>`. IDs continue after NEW-40 from round 4.

## Summary

Nadia can start a task and, after some confusion, allow the three workers and approve their changes from the inbox. Before pressing anything, though, she cannot tell Start from Start task. After pressing Start task she cannot see that four sessions exist: the session list has zero height in every run (NEW-41). Pressing Stop on the task stops only the coordinator, while three workers keep asking for approval (NEW-42). In a folder that is not a git repository, Start task looks like it worked ("Three workers are on it."). The only sign of trouble is three folded "Tool failed" rows that hide "Internal: git rev-parse: fatal: not a git repository" (NEW-43). Overall the flow works underneath, but the panel does not explain itself. A newcomer who reads every message literally gets several of them wrong: "You" in a worker's transcript, "Session s2", "finished and waits for you", and "Approved. Left for you: NOTES.md."

New: 0 blocker, 4 major, 6 minor, 2 polish.

## Findings

### NEW-41 major: The session list has zero height, so she never sees the coordinator and its three workers, and cannot pick a worker with the mouse

- **Area:** ui/src/agents (AgentsPanel layout)
- **Steps:** 1. Open demo-app. 2. Type "Add notes, tests and lint fixes" and click Start task. 3. Look for the sessions under the "Sessions" heading.
- **Expected:** Four rows: the task (coordinator) and the workers docs, tests and lint, each with a state.
- **Actual:** The "Sessions" heading sits directly on top of the coordinator's detail (`s4/31-all-allowed.png`, `s2/11-sessions.png`). `ul.sessions` measures 0 px tall, with a scrollHeight of 96 px (4 rows) at 1280x800. It is still 0 px at 1600x1000 (`s3/20-big.png`), with 5 sessions (scrollHeight 120), and even with one plain Start session (scrollHeight 24, run `s7`). Playwright could not click "Write the docs notes." because the heading and the detail cover it (`s2/failure.png`). The rows exist in the DOM and can only be reached with Tab.
- **Why it matters for Nadia:** "How many agents run, and which one wants what?" is her first question after Start task. The panel only ever shows the coordinator, and its status is "Waiting for you". She has no way to open a worker's transcript.
- **Suggested fix:** Give the list `flex: none` (or `min-height` for at least 3 rows) so the transcript shrinks instead of the list. Add a unit or e2e check that the session rows are visible after a task starts.

### NEW-42 major: Stop on a task stops only the coordinator; the three workers keep running and asking for approval

- **Area:** ui/src/agents (the core already has `orchestration.stop`)
- **Steps:** 1. Start task "Write notes". 2. The three "Write NOTES.md" cards appear. 3. Click Stop in the session detail. The coordinator is selected, because it is the only session she can see (NEW-41).
- **Expected:** The task stops: the coordinator and every worker, and their pending requests are denied. Or at least a choice between "Stop the coordinator" and "Stop the whole task".
- **Actual:** The UI sends `agents.stop {session: "s2"}`. The coordinator becomes "Stopped" and offers "Resume with a message". The workers stay "Needs approval", and Approvals still shows "(3 waiting)" (`s7/61-coordinator-stopped.png`). There is no button anywhere that calls `orchestration.stop`.
- **Why it matters for Nadia:** Stop is her emergency brake. She believes she stopped the task, but three agents keep working in her name. Because of NEW-41 she cannot even find them to stop them one by one.
- **Suggested fix:** When the selected session is a task's coordinator, the Stop button should stop the whole task (`orchestration.stop`) and say so ("Stop task: the coordinator and 3 workers"). Show each worker's Stop in its own row.

### NEW-43 major: Start task in a folder that is not a git repository looks like it worked; the error is a folded "Internal: git rev-parse: fatal: …"

- **Area:** crates/core/src/orchestration (check and wording; ui/src/agents for the up-front hint)
- **Steps:** 1. Open a plain folder (`my-site-*`, no `.git`). 2. Type "Add a contact page and tests" and click Start task.
- **Expected:** Before anything starts: "Start task needs a git repository, because each worker gets its own copy (a git worktree). Use Start to work in this folder, or run `git init` and commit first." Start task should be disabled with that hint, or the request should fail with that text and keep her prompt.
- **Actual:** `orchestration.start` succeeds and the prompt field is cleared. The coordinator's transcript shows three "Use mcp__ovra__spawn_agent" rows, each with a folded "Tool failed (1 line)". Unfolded, they say `Internal: git rev-parse: fatal: not a git repository (or any of the parent directories): .git`. The status is "Waiting for you", and the toast says "Add a contact page and tests finished and waits for you" (`s6/51-plain-start-task.png`). Approvals stays empty and nothing goes to Reviews. (The fake coordinator then says "Three workers are on it.". A real coordinator might explain the failure, but Ovra itself never tells her.)
- **Why it matters for Nadia:** Her personal projects often are not repositories. She waits for workers that will never come. If she does find the message, "Internal", "rev-parse" and "fatal" are exactly the codes that scare her.
- **Suggested fix:** Have `orchestration.start` reject a project without git with a plain, actionable message, and show it inline under the buttons (the existing `error` paragraph), with the prompt kept. Better still, disable Start task in such projects with the reason as its hint. The "Internal: …" text from `spawn_agent` should also be reworded for the agent ("This project is not a git repository; workers cannot be started").

### NEW-44 major: Before pressing either button, she cannot tell Start from Start task

- **Area:** ui/src/agents
- **Steps:** 1. Open demo-app and read the task box (`s1/02-typed-hover.png`).
- **Expected:** Two clearly different choices, for example "Start one agent here" and "Split into a task (up to 3 workers, each in a separate copy)", with the difference spelled out next to each button. The common choice should look primary.
- **Actual:**
  - The label says "Task for a new Claude session" for both buttons. "Task" names the field, yet only one button is "Start task".
  - The hint is a 75 px tall, 196 px wide paragraph squeezed next to the buttons: "Works in the project folder. Ctrl+Enter starts. Start task lets Claude split the work among up to 3 workers, each in its own worktree, for you to review." The first two sentences are about Start, but they read as if they were about both.
  - "Worktree" is git jargon she does not know. Nothing tells her that Start's changes land in her folder right away, while Start task's changes land only after she approves them in Reviews.
  - The two buttons look identical, except that "Start task" wraps to two lines (51x40 px against 51x24 px). Neither has a title. Ctrl+Enter silently means Start.
- **Why it matters for Nadia:** The choice decides whether agents write into her folder directly or into separate copies that she must review and copy back. She reads literally and picks by the name. "Start task" sounds like what she wants, because she typed a "task".
- **Suggested fix:** Give each button its own one-line description: a radio-like choice ("In this folder, one agent" / "As a task: up to 3 workers in separate copies; you approve each result in Reviews"), and one Start button below. Or put the descriptions under each button. Name the field "What should Claude do?". Write "separate copy of the project (git worktree)" the first time the word appears. Keep both buttons on one line.

### NEW-45 minor: Worker notifications say "Session s2 needs approval", and the agents' "finished and waits for you" toasts promise something they don't link to

- **Area:** ui/src/notify (notifications) with ui/src/agents (session names)
- **Steps:** 1. Start task. 2. Read the toasts (`s1/04-after-2s.png`, `s3/20-big.png`).
- **Expected:** "Worker docs (Add notes, tests and lint fixes) needs approval: Write NOTES.md". When a worker is done: "docs is ready for review", with a "Review" button that opens its inbox item.
- **Actual:** The first two workers show up as "Session s2 needs approval" and "Session s3 needs approval". The third, whose session info had loaded by then, shows "Write the lint notes. needs approval" (a race between the event and `agents.list`). Right after start, the coordinator's toast says "Add notes, tests and lint fixes finished and waits for you" while all three workers are still asking. Later, four "… finished and waits for you" toasts pile up with only a × (`s4/31-all-allowed.png`). None of them says the results are in Reviews.
- **Why it matters for Nadia:** "s2" means nothing to her. "Finished" makes her think the task is done before she has allowed anything. "Waits for you" does not say where.
- **Suggested fix:** Use the worker's name and the task title. Fall back to "an agent", never the raw id. When a worker asks for review, say "ready for review" and add a "Review" action that opens the inbox at that item. Do not call the coordinator "finished" while its workers run. Collapse the toasts of one task into one.

### NEW-46 minor: Nothing tells her that a task runs one coordinator plus up to three workers, or which worker is which

- **Area:** ui/src/agents (Transcript, session rows)
- **Steps:** 1. Start task. 2. Read the coordinator's transcript and the session rows (reach them with Tab, see NEW-41).
- **Expected:** The task shown as a group: "Add notes, tests and lint fixes: coordinator + 3 workers (docs, tests, lint)". Each worker row shows its name and where it works.
- **Actual:** The transcript shows three identical rows, "Tool mcp__ovra__spawn_agent / Use mcp__ovra__spawn_agent", each with a folded "Tool output (1 line)". The worker's name and job (`{"name":"docs","task":"Write the docs notes."}`) are not shown. The session rows are flat, with the workers titled after their prompt ("Write the docs notes.", trailing period included). The names docs, tests and lint that the core keeps (`Worker.name`, branch `ovra/t1/docs`) never appear in the UI. The panel's status line for the coordinator says "Waiting for you", although it is the workers that need her.
- **Why it matters for Nadia:** She cannot answer "how many agents run and what is each one doing".
- **Suggested fix:** Render `spawn_agent` calls as "Started worker docs: Write the docs notes." (and failures as "Could not start worker docs: …"). Group workers under their task in the list with their name. Give the coordinator a status like "Waiting for 3 workers".

### NEW-47 minor: A worker's transcript labels the coordinator's instructions as "You"

- **Area:** ui/src/agents (Transcript)
- **Steps:** 1. Start task. 2. Select the docs worker (with Tab and Enter, because of NEW-41) (`s3/21-worker-by-keyboard.png`).
- **Expected:** "From the coordinator: Write the docs notes."
- **Actual:** "You / Write the docs notes."
- **Why it matters for Nadia:** She never wrote that. Reading literally, she wonders whether her prompt was changed or sent somewhere else.
- **Suggested fix:** For workers, label the first user message as coming from the coordinator (or the task).

### NEW-48 minor: Approval cards say the worker writes "in demo-app … inside the agent's working folder", not in a separate copy

- **Area:** ui/src/approvals (the panel that renders the cards)
- **Steps:** 1. Start task. 2. Read a "Write NOTES.md" card (`s1/04-after-2s.png`).
- **Expected:** "docs (worker of 'Add notes, tests and lint fixes') wants to create NOTES.md in its own copy of demo-app. Your folder changes only when you approve its review."
- **Actual:** "Write the docs notes. in demo-app asks to use Write / Write NOTES.md / Inside the agent's working folder: NOTES.md". The three cards are word for word the same apart from the first line.
- **Why it matters for Nadia:** She thinks Allow writes NOTES.md into her project three times, and either denies out of caution or is surprised when it does not appear in the tree (it shows up only after Approve).
- **Suggested fix:** For workers, name the worker and say "in its own copy (worktree)". Add one line on what happens next ("then it asks for your review").

### NEW-49 minor: Approving a second worker that touched the same file says "Approved. Left for you: NOTES.md." and the item is marked Approved

- **Area:** ui/src/inbox (outcome wording; crates/core/src/orchestration for the data)
- **Steps:** 1. Start task, allow all three. 2. Reviews. Each item says "1 file, 1 also in another open review". 3. Approve docs. 4. Approve lint.
- **Expected:** Before approving: "NOTES.md was already changed by docs, which you approved. Approving lint will not copy it." After approving: "Not copied: NOTES.md (already changed in your folder). Compare or copy by hand: …".
- **Actual:** The lint item turns "Approved", and the line reads "Approved. Left for you: NOTES.md." The list entry says "Approved / 1 file" with no marker (`s5/41-approved-2.png`). Nothing says whether lint's version was copied, merged or dropped. As soon as docs is approved, the "also in another open review" markers on the other items vanish, so the warning disappears before the risky step.
- **Why it matters for Nadia:** "Left for you" is ambiguous: left for her to do what? She will believe all three workers' notes are in her project.
- **Suggested fix:** Say what happened in plain words ("Not copied because your copy changed: NOTES.md"). Mark such items as "Approved, 1 file not copied". Keep the overlap marker as "already changed by an approved review".

### NEW-50 minor: The inbox explanation uses git words she does not know

- **Area:** ui/src/inbox
- **Steps:** 1. Open a worker's item (`s4/32-inbox.png`).
- **Expected:** "Approve copies this worker's changes into your project folder. It does not commit them unless you tick the box."
- **Actual:** "Approve copies the worker's changes into your checkout. Merging its branch stays with you." Below that is a "Commit the agent's files" checkbox. The items show neither the task they belong to nor the worker's name ("Worker: Write the docs notes.").
- **Why it matters for Nadia:** "Checkout", "merging its branch" and "stays with you" read as a chore she does not know how to do, so she worries that Approve is incomplete.
- **Suggested fix:** Use "your project folder". Mention the branch only as an optional detail ("Its branch ovra/t1/docs stays, if you want to merge it yourself"). Group the items under the task title.

### NEW-51 polish: Starting a task clears the prompt even when the task cannot do anything

- **Area:** ui/src/agents
- **Steps:** NEW-43's steps. Afterwards, look at the task box.
- **Expected:** Her text stays until at least one worker has started, or the error is shown, so that she can click Start instead.
- **Actual:** The field is empty right after the click (`s6/51-plain-start-task.png`).
- **Why it matters for Nadia:** To recover she has to retype the task.
- **Suggested fix:** Clear the prompt only after a successful start. With NEW-43's up-front check, this comes for free.

### NEW-52 polish: Approvals takes 60% of the column while workers wait, and pushes the Agents panel off screen

- **Area:** ui/src/shell (right column layout)
- **Steps:** 1. Start task at 1280x800 (`s1/04-after-2s.png`).
- **Expected:** Three identical requests stay compact, for example "3 workers want to write NOTES.md", with Allow all / review each.
- **Actual:** Approvals is 475 px tall and the Agents panel 265 px (scrollHeight 893). Only the setup cards are visible, not the task or its sessions.
- **Why it matters for Nadia:** She loses sight of the task she just started at the moment she most wants to watch it.
- **Suggested fix:** Collapse each card's details (file and content) after the first one, or group identical requests by task. Cap Approvals at about half the column.

## What worked well

- Start task reacts instantly: the coordinator appears and the three approval cards follow in well under a second.
- The approval cards are safe to use. The 500 ms arm delay and the Y/N hints work for workers just as for direct sessions.
- The Reviews button counts correctly ("Reviews (3 waiting)"). Each inbox item shows the agent's summary, the file list and a clear diff. Approve really copies NOTES.md into her folder, and it appears in the tree.
- The overlap warning ("1 also in another open review") is a good idea. It only needs to survive the first approve (NEW-49).
- A git repository without any commit (`empty`) works end to end: the workers start, she approves, and NOTES.md lands in the folder.
- Worktrees are kept outside her project folder, so the file tree stays clean while workers run.
