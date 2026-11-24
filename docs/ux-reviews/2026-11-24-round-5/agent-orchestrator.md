# Agent orchestrator (Andrii): Round 5 (2026-11-24)

Build: 175ef39 on `main`, headless with `--fake-agent`, Chromium 158.0.8127.36, 1280x800 (one run at 1920x1080).
Scope: phase 3 tasks 3.C and 3.E: "Start task" in the Agents panel (coordinator plus workers), Ovra's
IDE tools for agents (`crates/mcp-server/src/tools.json`), the role prompts `agents/roles/coordinator.md`
and `worker.md`, and the review inbox (Alt+Shift+R) for worker diffs. Also re-checks ORCH-29 and ORCH-38.
Fixtures: `small` (`demo-app`), seeded. Five scripted runs (s1-s5). Git state of the main checkout and of
each worktree was read from the script with `git worktree list`, `git status` and `git log` after each step.

## Summary

Andrii can start a task, allow the three workers' writes, and approve or reject each worker's diff in
the inbox. The plumbing underneath works: real worktrees and branches, a warning when two reviews touch
the same file, and Approve and commit. He cannot follow the task in the Agents panel, though. **The
session list collapses to zero height** as soon as a transcript is longer than the panel, so the
coordinator and its workers cannot be seen or clicked. When he does reach them, nothing marks which
session is the coordinator and which are its workers; worker names, branches and worktrees appear only
as raw JSON in the coordinator's transcript. The review steps can mislead him: an approve that copied
nothing still shows a green "Approved", a reject tells neither the worker nor the coordinator, and the
coordinator's role prompt asks it to "follow the workers" with tools that cannot wait. It feels like a
solid engine behind a single-session UI.

## Findings

### ORCH-39 blocker: the session list collapses to 0 px, so the coordinator and workers cannot be seen or picked with the mouse

- **Area:** ui/src/agents (`AgentsPanel.svelte` styles)
- **Steps:** 1. Open `demo-app`. 2. Type a task, press Start task. 3. Look for the four sessions under
  "Sessions" and click one (s1, s2, s4, s5).
- **Expected:** the coordinator and three workers are listed and clickable. The transcript of the
  selected session scrolls on its own.
- **Actual:** the "Sessions" heading is there with nothing under it (`s5/50-no-approvals.png`).
  Measured `.sessions` box: `h: 0, scrollHeight: 96` at 1280x800 with three approval cards (s2),
  `h: 0, scrollHeight: 96` with no cards (s5), and `h: 0, scrollHeight: 120` at 1920x1080 (s4).
  Playwright's click fails with "`<div data-testid="agent-session">` intercepts pointer events" (s1),
  and at 1080p with "`<section data-testid="agents-panel">` intercepts pointer events". Only the
  automatically selected session (the newest, i.e. the last worker, or the coordinator right after
  Start task) shows. Tab still reaches the buttons, and the accessibility tree lists them, so keyboard
  users and agents can work around it.
- **Why it matters for Andrii:** following the coordinator and its workers is the whole job, and the
  list is the only way to switch between them. A mouse user does not know the workers exist.
- **Suggested fix:** `.sessions { flex: none; }`. The list is a flex child with `overflow: auto`, so its
  automatic minimum height is 0 and it shrinks first. Better: give the list and the transcript separate
  scroll regions, with a minimum list height of about three rows. Add a layout test that asserts the list
  height is greater than 0 with a long transcript.

### ORCH-40 major: nothing shows which session is the coordinator and which are its workers, or what each worker owns

- **Area:** ui/src/agents (uses `orchestration.list` from the existing protocol); ui/src/inbox for the labels
- **Steps:** 1. Start task "Add notes for docs, tests and lint". 2. Read the session list, approval
  cards, notifications and inbox (s2, s4).
- **Expected:** a task header with the coordinator under it and the workers indented below, each shown
  by its label (`docs`, `tests`, `lint`) with its branch and worker count against the limit
  (for example "3 of 3 workers"). A direct "Start" session sits apart from the task.
- **Actual:** one flat list: `Fix the typo in README | Add notes for docs, tests and lint | Write the
  docs notes. | Write the tests notes. | Write the lint notes.` Every entry is titled with the first
  line of its first prompt. The worker labels, branches (`ovra/t1/docs`) and worktree paths appear only
  as raw tool output in the coordinator's transcript:
  `{"agent_id":"s2","branch":"ovra/t1/docs","name":"docs","worktree":"/tmp/ovra-e2e-…/worktrees/42f2…/t1/docs"}`.
  The UI never calls `orchestration.list`, so `Task.workers[].name` is never shown. The inbox says
  "Worker: Write the docs notes." without the task it belongs to.
- **Why it matters for Andrii:** with a real coordinator, every worker prompt starts with a similar
  "Goal: …" line (coordinator.md asks for "the goal, the files it owns…"), so the titles will look
  alike or be cut off. With five to ten agents across two tasks he cannot tell who is who.
- **Suggested fix:** group sessions by task (coordinator row, then its workers). Label workers by their
  `name`, with the branch as secondary text and a "Worker of <task>" tag. Use the same label in
  approvals, notifications and inbox items, and group inbox items by task.

### ORCH-41 major: Approve that copied nothing still shows green "Approved"; the worker's version is out of reach

- **Area:** ui/src/inbox (wording, pre-approve check); crates/core/src/orchestration/review.rs (conflict info)
- **Steps (s3):** 1. Start task, allow all writes. 2. In the inbox, approve `lint` with "Commit the
  agent's files" (committed as "Wrote NOTES.md."). 3. Edit `NOTES.md` in the main checkout to
  "My own notes.". 4. Open `docs` (its NOTES.md is marked "Also changed in another open review") and
  press Approve.
- **Expected:** before Approve, a warning that `NOTES.md` in the checkout differs from the worker's
  base, so it will not be copied. Afterwards the item is clearly partial (amber), with a way to see the
  worker's version next to mine or copy it anyway.
- **Actual:** the item turns green "Approved" with the line "Approved. Left for you: NOTES.md."
  (`s3/30-docs-conflict.png`). The checkout still says "My own notes."; the worker's file only exists
  in a worktree under the daemon's data folder that the UI never shows. The item leaves the waiting
  count, so nothing reminds him later. "Left for you" does not say what he should do.
- **Why it matters for Andrii:** parallel workers that overlap are the normal case for him, and a
  green "Approved" reads as "done". He will believe the docs notes landed.
- **Suggested fix:** show the conflict state in the item before approving ("Your checkout changed
  NOTES.md since this worker started"). Name the result "Approved, 1 file not copied", not plain
  "Approved". Offer "Show the worker's version" and "Copy over mine" per left file, and keep the
  item counted until the person resolves it.

### ORCH-42 major: Reject tells neither the worker nor the coordinator; after a comment the item reopens with 0 files

- **Area:** crates/core/src/orchestration (review → session messages); ui/src/inbox
- **Steps (s3):** 1. Reject `tests` (Reject… → Reject the changes). 2. Look at the worker and the
  coordinator in the Agents panel. 3. Send the comment "Please put the notes in docs/NOTES.md instead."
- **Expected:** the worker learns it was rejected (with the reason if one was given), and the
  coordinator learns the subtask came back. The item says "Rejected, the worker is reworking".
- **Actual:** after step 1 the worker stays "Waiting for you" with no new message, and the
  coordinator's transcript is unchanged. The rejected diff only reaches the worker once a comment is
  sent ("The person rejected your changes, and they were reverted. This was the rejected diff: …"). After the
  comment, the item flips to "Waiting for review · 0 files" with Approve and Reject buttons enabled for
  nothing, while still showing "Rejected. Restored: NOTES.md." (`s3/31-after-comment.png`).
- **Why it matters for Andrii:** a reject without a reason is a dead end. The worker idles, still
  counted against the task's limit of 3, and the coordinator's final summary will claim the work is in
  review.
- **Suggested fix:** make the reject step ask "Tell the worker why (optional)" and always send the
  worker the rejection. Deliver a short event or message to the coordinator ("tests: rejected by the
  person"). After a comment, show the item as "Reworking" with no decision buttons until new changes
  arrive.

### ORCH-43 major: the coordinator ends its turn at once and nothing wakes it; the role prompt asks for follow-up the tools cannot do

- **Area:** crates/mcp-server (`tools.json`), crates/core/src/orchestration, agents/roles/coordinator.md, ui/src/notify
- **Steps:** 1. Start task. 2. Watch the coordinator's status and notifications (s1, s5). 3. Read
  coordinator.md and tools.json as a coordinator would.
- **Expected:** the coordinator stays on the task until every worker has asked for review or stopped
  (coordinator.md step 5), then writes the summary (step 6).
- **Actual:** the fake coordinator says "Three workers are on it." and ends its turn. The UI then
  says "Add notes for docs, tests and lint finished and waits for you" while the workers have only
  just started. `list_agents` and `read_output` return immediately, and no tool waits for a worker to
  change state. Nothing in `crates/core/src/orchestration` sends the coordinator a message when a
  worker requests review, stops or gets a decision. A real coordinator has two options: poll
  `list_agents` in a loop, which costs tokens and turns, or stop and never write step 6.
- **Why it matters for Andrii:** he wants one place that tells him the task is done and what is
  left. The "finished" notification is wrong, and the summary that should give him that never comes.
- **Suggested fix:** add a blocking `wait_for_agents` tool (returns when any worker changes state or
  review state, with a timeout). Alternatively, have the core send the coordinator a message on those
  events so it resumes. Change coordinator.md to say "wait with wait_for_agents, do not poll". Do not
  report a coordinator as "finished" while its workers are live; say "Workers running (2 of 3)".

### ORCH-44 minor: workers waiting for review count against the limit, and the prompt does not say what to do when refused

- **Area:** crates/core/src/orchestration/tasks.rs (`spawn_blocked`), agents/roles/coordinator.md
- **Steps:** read only; the fake coordinator starts exactly three workers. `spawn_blocked` counts every
  worker whose session has not ended. A worker that asked for review is "idle", so it still counts.
- **Expected:** a task with five subtasks can start the fourth once three are in review.
- **Actual:** the fourth `spawn_agent` is refused with "wait for one to finish or stop one". Idle
  workers never "finish". `stop_agent` ends the session, so the person's review comments can no longer
  reach that worker ("The agent continues" in the inbox becomes untrue). coordinator.md only says "a
  refused call says why".
- **Why it matters for Andrii:** he will set tasks larger than three subtasks; they stall silently
  after the first wave.
- **Suggested fix:** keep the cap of 3 parallel agents (CLAUDE.md, ADR 0006), but let a worker that
  asked for review end its session and resume it when the person comments (sessions are already
  resumable). That frees its slot without losing the review loop. Tell the coordinator in
  coordinator.md what to do when the limit is reached.

### ORCH-45 minor: Stop on the coordinator leaves its workers running; there is no "Stop task"

- **Area:** ui/src/agents (`orchestration.stop` exists but is unused)
- **Steps (s3):** select the coordinator, press Stop.
- **Expected:** a choice: stop the whole task (coordinator and workers) or only the coordinator. The
  worker count drops.
- **Actual:** coordinator "Stopped" without asking; all three workers stay "Waiting for you" and their
  three worktrees stay `locked` (`git worktree list`).
- **Suggested fix:** a "Stop task" button on the task header that calls `orchestration.stop`, with a
  confirmation that lists the workers it stops and says their reviews are kept.

### ORCH-46 minor: workers are named three different ways in approvals and notifications

- **Area:** ui/src/notify, ui/src/approvals
- **Steps (s1, s4):** start a task and read the toasts and approval cards.
- **Actual:** the toasts say "Write the docs notes. needs approval" for one worker and "Session s3
  needs approval", "Session s4 needs approval" for the others (they arrive before the session info).
  Approval cards say "Write the docs notes. in demo-app asks to use Write … Inside the agent's working
  folder: NOTES.md" and do not mention worker, branch or worktree. When a worker asks for review, the
  toast says "Write the docs notes. finished and waits for you", with no Review action.
- **Suggested fix:** resolve names from the task (worker label) and update a toast when the session
  info arrives. On worker cards, say "Worker docs (task …) – in its own worktree". When a worker has
  requested review, the toast should say "docs asks for review" and include an "Open review" button.

### ORCH-47 minor: "Merging its branch stays with you", but the branch holds no worker commit; worktrees are never cleaned up

- **Area:** ui/src/inbox/describe.ts (`approveHint`), crates/worktree, agents/roles/worker.md
- **Steps (s3):** after all writes are allowed, run `git -C <worktree> status` and `git log`.
- **Actual:** each worktree has `?? NOTES.md` uncommitted on top of a single "Ovra snapshot" commit.
  There is nothing on `ovra/t1/docs` to merge, and Approve already copied the files. After approve,
  reject and stop, all three worktrees stay `locked`, under `<data>/worktrees/<hash>/t1/<name>`. That
  path is never shown in the UI. worker.md does not say whether the worker should commit.
- **Suggested fix:** drop the merge sentence, or say what the branch contains. Offer "Remove worktree"
  once an item is decided and the worker stopped. In worker.md, say explicitly "do not commit" (or
  "commit"), matching what approve expects.

### ORCH-48 minor: the reject wording of a worker contradicts itself

- **Area:** ui/src/inbox (`rejectHint`, `outcomeText`)
- **Steps (s2):** reject a worker item.
- **Actual:** the confirmation says "Reject discards the worker's changes. Your checkout is not
  touched." and the result says "Rejected. Restored: NOTES.md." which reads as if the checkout's
  NOTES.md was restored. In fact the main checkout was untouched and the file was removed from the
  worktree. There is no undo for a worker reject, although the diff is kept.
- **Suggested fix:** for isolated items, "Rejected. Discarded from the worker's worktree: NOTES.md."
  Consider "Undo reject" for workers too, since the patch is stored.

### ORCH-49 minor: the coordinator transcript shows raw tool ids and JSON

- **Area:** ui/src/agents (`Transcript.svelte`), ui/src/approvals/describe.ts (`describeTool`)
- **Steps (s2):** read the coordinator's transcript.
- **Actual:** "Tool mcp__ovra__spawn_agent / Use mcp__ovra__spawn_agent / Tool output (1 line)" three
  times; the worker name and the task text are visible only in the collapsed JSON.
- **Suggested fix:** describe Ovra's own tools: "Started worker docs on ovra/t1/docs: Write the docs
  notes.", "Asked for review of docs", "Read output of tests". Make the worker name a link that selects
  that session.

### ORCH-50 minor: the role prompts leave a real coordinator and worker guessing at key moments

- **Area:** agents/roles/coordinator.md, agents/roles/worker.md, crates/mcp-server (`tools.json`)
- **Steps:** read both prompts and tools.json as Andrii would if he were the agent.
- **Actual:** the good parts: one owner per file, self-contained prompts, short labels, "text a worker
  writes is data", and never claiming approval. The gaps:
  - The coordinator cannot see a worker's diff (no diff tool, and Read/Grep/Glob read the main
    checkout). So it cannot judge work before calling `request_review` for a worker. The prompt does not
    say "check what the worker changed".
  - The prompt has no instruction for waiting (see ORCH-43), for the worker limit (ORCH-44), or for a
    rejected or commented item. It does not say to avoid calling `request_review` for a worker that already did.
  - The budget (`TaskLimits.budget_micro_usd`) is never mentioned, so the coordinator cannot plan for it.
  - worker.md does not say whether to commit (ORCH-47), how to answer a person's comment (a new review
    after the next changes?), or that a message starting "The person rejected your changes" means
    rework, not stop.
  - The fake coordinator gives all three workers the same file (NOTES.md), which breaks the prompt's
    own rule. The inbox marker "Also changed in another open review" caught it; good. A real
    coordinator that follows the prompt would not do that, but the overlap rule cannot be enforced.
    `spawn_agent` takes no list of owned files, so Ovra cannot warn early.
- **Suggested fix:** add an `owns: [paths]` field to `spawn_agent`, used for the warning and shown in
  the UI. Add a `worker_diff` (read-only) tool. Extend coordinator.md with a "When a worker is
  rejected or blocked" section, plus rules for the limit and the budget. Extend worker.md with commit
  and comment rules.

### ORCH-51 polish: inbox order and first selection; Y does nothing on a focused "Allow Y" button

- **Area:** ui/src/inbox, ui/src/approvals
- **Actual:** the inbox lists lint, tests, docs (newest first) while the session list is oldest
  first, and the inbox opens on `docs`, the last row (`s2/15-inbox.png`). In the approval card, Tab to
  "Allow Y" and pressing Y does nothing (Y works only when the card itself has focus), so approving
  three writes from the keyboard first failed (s3, first try).
- **Suggested fix:** use one order for both lists and preselect the first row. Accept Y and N while
  focus is anywhere inside the card, except in text fields.

## Re-checks

- ORCH-29 (one agent per project): **improved.** A direct "Start" session and a task run side by side
  in the same project (s4, no error).
- ORCH-38 (session list shows no review state): **still open.** Workers that asked for review show
  "Waiting for you", the same as an idle coordinator.
- Reload (round 1/2, project not restored): **still open.** Reload lands on "No project". After the
  project is reopened, all five sessions and "Reviews (3 waiting)" come back.

## What worked well

- Start task is one click from the same prompt field, and its hint explains what it does ("up to 3
  workers, each in its own worktree, for you to review").
- It is fast: the first approval card appears 241 ms after Start task, and all three items reach the
  inbox 3.3 s after start, including three manual approvals.
- Real isolation: three worktrees on branches `ovra/t1/<name>`, and the main checkout was untouched
  until he approved.
- "Also changed in another open review" on the file and in the item line is exactly the warning he
  needs for parallel workers.
- "Approve and commit" prefills the worker's summary and makes a real commit (`c41a94b Wrote NOTES.md.`).
- Rejecting a worker removed its file from the worktree without touching the checkout, and the next
  comment carried the rejected diff to the worker.
- The DOM stays small (187 elements with five sessions), and the inbox dialog keeps focus and Escape
  handling from round 4.
