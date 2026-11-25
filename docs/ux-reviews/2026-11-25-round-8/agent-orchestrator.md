# Andrii, the agent orchestrator: round 8

Build: 2236c61, headless (`just build-headless` with `CARGO_TARGET_DIR=/tmp/ovt-px-orc`),
Playwright 1.63 Chromium (build 1243), 1280x800 (one check at 1024x640).
Scope: the redesigned Agents column (task rows, workers, capacity header,
progress meter), the transcript (folded tool cards, Jump to latest, turn
divider, Retry), grouped approval cards, the approvals toast, the banner over
a modal, Reviews in the title bar. Fixtures: `small` (seeded).
Agents: the scripted fake agent (`--fake-agent`) for tasks, workers and long
sessions; the real driver against `fake-claude` (`edit_allowed`, and a copy of
it whose second turn ends with `error_max_turns`) for turn dividers, costs,
errors and Retry. The harness was a scratch copy of `tests/e2e/ux/run.ts` whose
only change is the binary folder (`/tmp/ovt-px-orc/debug`).
Screenshots: `/mnt/project-files/ovra/screenshots/round-8/orch-*.png`.

## Summary

Andrii can start a Plan and split task, answer every request and reach the
three reviews; the header ("3 of 3 · 3 queued"), the meter ("2 of 3 done") and
the folded one-line tool cards are the right ideas and work. The worst problem
is room: at 1280x800, while approvals wait, the coordinator's transcript gets
32 px, the task list 72 px with 0 of 8 rows fully in view, and on a real
Claude request Allow once sits below the approvals' own scroll edge; on top of
that, a sandboxed session spends about 290 px on its title and sandbox gap
list before the first message. Second tier: Retry never appears after a real
turn error, tool lines show absolute paths cut before the file name, and
agents that only wait for him fill the 3 slots and block a new "One agent,
here" task.

## Findings

### ORCH-1 major: while approvals wait, the Agents column has no room for the transcript or the task list

- **Area:** ui/src/shell (column layout), ui/src/approvals, ui/src/agents
- **Steps:** 1. 1280x800, open `demo-app`. 2. Give the task "Write the notes",
  Plan and split, Start. 3. Three worker requests arrive. 4. Look at the
  coordinator, then press Tasks. 5. Start a second Plan and split task and
  press Tasks again.
- **Expected:** with requests waiting, Andrii still sees the task's last few
  messages and every task and worker row, at least as one line each.
- **Actual:** the approvals block takes 368 px (36-404). Under it, the Agents
  header, the one-time hint "Allow lets Claude do this one step…" (2 lines)
  and the session facts leave the transcript a box of 320x32 px: only "Three
  workers are on it." is visible; "Started worker docs/tests/lint" are out of
  sight (`orch-02-coordinator-squeezed.png`). In the list, `.scroll` is 72 px
  high with `scrollHeight` 712: rows 0-7 are all at least partly cut, 0 of 8
  fully visible; only the coordinator row and its "0 of 3 done" show, the
  workers that need him are below (`orch-03-list-workers-hidden.png`,
  `orch-06-queued-header.png`). The start box under the list even clips its
  own hint ("…without a s…"). A worker opened while it waits shows only its
  last step (`orch-16-worker-view.png`).
- **Why it matters for Andrii:** the moment he is needed is the moment he
  can see least of what the agents did and which agent asks. With five to ten
  agents this is the normal state, not an edge case.
- **Suggested fix:** give the approvals a cap of about a third of the column
  (one card visible, a "2 more" line for the rest), and let the start box fold
  to one line ("Give Claude a task…") while a session or approvals are on
  screen. Drop the one-time Allow/Deny hint once a card is visible (the card
  explains itself). Then check that the transcript keeps at least 200 px and
  the list at least 4 rows at 1280x800 in an e2e layout test.

### ORCH-2 major: Allow once is hidden below the approvals' scroll edge on a single real request

- **Area:** ui/src/approvals
- **Steps:** 1. Run with `--claude edit_allowed`. 2. Start the recorded task
  ("Fix the typo in README.md with the Edit tool…"). 3. Look at the one
  request card.
- **Expected:** a single request shows its Deny and Allow once buttons without
  scrolling.
- **Actual:** the group header takes 3 lines (the long task title, wrapped,
  plus "Sandboxed · Billed to Claude plan"), the heading is the absolute path
  `Edit /tmp/ovra-ux-q8M0ID/demo-app/README.md` on 2 lines and the details
  repeat the path on 3 lines. The card list is an inner scroller (`UL`,
  `scrollHeight` 425, `clientHeight` 320); Allow once is at y=428, the
  approvals box ends at y=404, and `elementFromPoint` at its centre is not the
  button (`orch-15-allow-clipped.png`). Y still works from the keyboard.
- **Why it matters for Andrii:** he answers with the mouse when skimming; a
  card that shows the request but not the answer looks stuck, and he scrolls a
  320 px box inside a 764 px column.
- **Suggested fix:** keep the actions of the first card always in view (sticky
  footer of the approvals block, or put the buttons before the details). Shorten
  the group header to one line (ellipsis on the task title, its full text in a
  tooltip) and show the path relative to the project in the heading ("Edit
  README.md"); the scope line already says "In the project".

### ORCH-3 major: Retry never appears after a turn error from the real driver

- **Area:** ui/src/agents (Transcript.svelte, `lastError`)
- **Steps:** 1. Run with a copy of `edit_allowed.jsonl` whose second `result`
  has `"subtype":"error_max_turns","is_error":true`. 2. Allow the Edit; reply
  with the recorded second message; allow the Bash command.
- **Expected:** the error "The turn stopped: it reached its turn limit." has a
  Retry button.
- **Actual:** the error card is followed by the divider "Turn ended · 43.6k
  tokens" and no Retry anywhere (`retry count=0`,
  `orch-14-turn-error-no-retry.png`). The driver sends `Error` then `TurnEnd`
  with usage; `lastError` only matches when the error is the very last shown
  entry, which is true only for the fake agent, whose turn ends carry no usage
  and are filtered out.
- **Why it matters for Andrii:** turn limits, budgets and network errors are
  exactly where he wants one press to go on; today he has to retype or copy
  his last message.
- **Suggested fix:** treat the error as last when only `turn_end` entries
  follow it; add a Transcript unit test with `[error, turn_end(usage>0)]`.

### ORCH-4 major: agents that only wait for him take all 3 slots, and "One agent, here" is refused after the press

- **Area:** crates/core (capacity), ui/src/agents
- **Steps:** 1. Start a Plan and split task; its three workers ask to write
  NOTES.md and wait. 2. Header says "3 of 3". 3. Start "Fix the typo" as One
  agent, here. 4. Also start a second Plan and split task.
- **Expected:** the header says the three agents wait for him, and a new task
  either queues (like the split task's workers do) or Start says before the
  press why it cannot run.
- **Actual:** the header says "3 of 3" (tooltip "Agents working in this
  project") while all three show "Needs approval". The second split task's
  workers queue ("3 of 3 · 3 queued", "Workers are waiting for a free agent"),
  but the direct task fails after the press with "Could not start: 3 agents are
  already working in this project, the most at once…" (frame:
  `{"code":"conflict","message":"3 agents are already working…"}`,
  `orch-10-direct-refused-toast.png`). Start was enabled and the footer said
  nothing beforehand.
- **Why it matters for Andrii:** he is the bottleneck, and the cap tells him
  agents are "working" when they are blocked on him; two start modes behave
  differently at the same limit.
- **Suggested fix:** say "3 of 3 · 3 need you" (and use that wording in the
  tooltip); queue direct tasks the same way as workers, or add capacity to
  `startBlock` so Start explains "3 agents running; it will wait" before the
  press. Whether waiting agents should count against the cap is a product
  choice for the owner.

### ORCH-5 major: tool lines and approval headings show absolute paths, cut before the file name

- **Area:** ui/src/agents (steps.ts `verbAndObject`), ui/src/approvals (describe.ts)
- **Steps:** 1. Run the `edit_allowed` session. 2. Read the transcript after
  the first turn.
- **Expected:** "Read README.md", "Edited README.md +1 −1".
- **Actual:** "Read /tmp/ovra-ux-GqRfje/demo-app/R…", "Edited
  /tmp/ovra-ux-GqRfje/de… +1 −1" (`orch-13-turn-divider-abs-paths.png`). The
  ellipsis eats the file name, the one part he needs. Real Claude always sends
  absolute paths, so this is every session, not only the fixture.
- **Why it matters for Andrii:** the folded one-line steps are the
  readability feature; with the file name cut off he has to open each step.
- **Suggested fix:** show paths relative to the session's folder (project or
  worker copy) in step objects and approval headings, full path in the
  tooltip; if it must be cut, cut from the left ("…/demo-app/README.md").

### ORCH-6 major: the session's sandbox facts push the chat off screen

- **Area:** ui/src/agents (AgentsPanel `.info`, sandbox.ts)
- **Steps:** 1. Run the `edit_allowed` session (it runs in the Linux
  sandbox). 2. Open it.
- **Expected:** the title, the status and one sandbox line; gaps behind a
  click.
- **Actual:** the title wraps to 3 lines (the whole prompt), then "Done",
  "Claude plan", "Runs in a sandbox (bubblewrap, Landlock, seccomp), network
  only through Ovra's proxy." (2 lines) and "Not covered: git config values
  that name project files, submodules … CLAUDE.md or .envrc." (5 lines), then
  Details: about 290 of the 560 px between the bar and the reply box, every
  time the session is opened (`orch-13-…`, `orch-14-…`). With a request
  waiting, the transcript is not visible at all (`orch-15-allow-clipped.png`).
- **Why it matters for Andrii:** the owner wants a readable chat; this is a
  fixed wall of text above it in every sandboxed session.
- **Suggested fix:** one line "Sandboxed · gaps" with the gap list under
  Details (where the billing text already is); clamp the title to one line.
  The approval card's "Sandbox has gaps" already does this.

### ORCH-7 major: worker rows do not say what each worker does

- **Area:** ui/src/agents (sessions `name`, row title)
- **Steps:** 1. Start a Plan and split task. 2. Press Tasks.
- **Expected:** each worker row says its own assignment ("docs: Write the docs
  notes").
- **Actual:** rows read "docs · Write the no…", "tests · Write the no…",
  "lint · Write the notes": the parent task title, truncated, plus the worker
  name; the tooltip adds the branch, not the assignment. The assignment is
  only in the coordinator's transcript after expanding "Started worker docs",
  or as the first message inside the worker (`orch-04-all-done-list.png`,
  `orch-17-…`).
- **Why it matters for Andrii:** "what is each one doing" is the first
  question with several workers; today it costs one click per worker.
- **Suggested fix:** under a task, show the worker's name and its `task`
  (from `spawn_agent`) instead of repeating the parent title; keep the parent
  title on the task row only.

### ORCH-8 minor: Hide on the approvals leaves an empty block and the toast comes late

- **Area:** ui/src/approvals, ui/src/notify
- **Steps:** 1. With 3 requests waiting, press Hide on Approvals. 2. Wait.
  3. Click anywhere else.
- **Expected:** the approvals shrink to their header line, and the toast "3
  requests need approval · Review" appears at once.
- **Actual:** the header stays with about 190 px of empty space under it; the
  list rows stay cut (`orch-07-hide-empty-space.png`). No toast 300-400 ms
  after Hide; it appears only after the next focus change (the toast check
  runs on `focusin`/resize, and Hide keeps focus).
- **Why it matters for Andrii:** he hides the cards to get room back and does
  not get it; the reminder depends on where he clicks next.
- **Suggested fix:** let the collapsed section take only its header height;
  bump the notifier's `look()` when the approvals are collapsed or expanded.

### ORCH-9 minor: the coordinator's transcript repeats plumbing

- **Area:** ui/src/agents (Transcript.svelte)
- **Steps:** 1. Finish a Plan and split task. 2. Open the coordinator.
  3. Expand "Started worker docs".
- **Expected:** one line per worker: started, and later "asked for review".
- **Actual:** the expanded step ends with "What Claude got:" and raw JSON
  (`agent_id`, a `/tmp/…/worktrees/…` path); then three full grey cards "Ovra
  / Worker docs asked for review. / What Claude was told" in a row
  (`orch-05-coordinator-final.png`, `orch-17-…`).
- **Why it matters for Andrii:** three workers already take 6 blocks; ten
  workers would be a wall.
- **Suggested fix:** make the review notes from Ovra one-line steps (like tool
  cards) and fold consecutive ones as reads are folded ("3 workers asked for
  review"); hide the raw `spawn_agent` result behind "What Claude got" with the
  plain sentence above it only.

### ORCH-10 minor: "is done" toast stays while the session works again, and contradicts "Failed"

- **Area:** ui/src/notify (agent-events.ts)
- **Steps:** 1. Direct session, allow the first steps. 2. Reply; it asks
  again. 3. In the error session above, look at status and toast.
- **Expected:** a "done" toast leaves when the session starts its next turn;
  a turn that ended in an error is not "done".
- **Actual:** "Fix the typo is done." stays for more than 10 s while the
  status says "Needs approval" (`orch-12-jump-to-latest.png`). After the
  turn-limit error the status reads "Failed" and the toast "… is done."
  (`orch-14-…`).
- **Why it matters for Andrii:** he triages from toasts; a stale or wrong one
  sends him to the wrong agent.
- **Suggested fix:** dismiss `session:<id>` when the session becomes busy;
  word a turn that ended in an error as "stopped: <reason>" with Retry/Open.

### ORCH-11 minor: the "N changes wait for your review" card sits out of line

- **Area:** ui/src/agents (AgentsPanel `.card.outcome`)
- **Steps:** 1. Finish a Plan and split task. 2. Open the coordinator.
- **Expected:** a card aligned with the rest, with a visible Review button.
- **Actual:** the white card runs from x=968 to the column's right edge, the
  text is centred and Review is plain text; it covers the transcript's first
  line ("Started worker lint" shows half under it)
  (`orch-05-coordinator-final.png`).
- **Suggested fix:** use the same margins as the other cards, left-aligned
  text, and a real button ("Review 3 changes").

### ORCH-12 minor: focus falls to the page body after the last approval

- **Area:** ui/src/approvals
- **Steps:** 1. Alt+Shift+A with 3 requests. 2. Y, Y, Y.
- **Expected:** focus goes back to where he was (or to the task row).
- **Actual:** after the third Y `document.activeElement` is `BODY`. The rest
  worked well: 3 requests answered in 1.9 s, an early Y says "Ready in a
  moment — read the request first."
- **Suggested fix:** return focus to the element focused before the shortcut,
  or to the Agents heading.

### ORCH-13 polish: a finished coordinator still offers Stop next to Close

- **Area:** ui/src/agents
- **Steps:** open a task whose status is "Done".
- **Actual:** the bar shows Stop and Close (`orch-05-…`); what Stop does to a
  done task (and its workers) is not said.
- **Suggested fix:** for an idle coordinator show Close only, or rename Stop
  to "End task" with a tooltip.

### ORCH-14 polish: the approvals banner covers the title-bar search

- **Area:** ui/src/notify
- **Steps:** with requests waiting, open the palette (Ctrl+K) or the Review
  inbox (Alt+Shift+R).
- **Actual:** "3 approvals wait · Show" is drawn over "Search or run"
  (`orch-08-banner-over-palette.png`, `orch-09-banner-over-inbox.png`). It
  works and Show leads to the cards.
- **Suggested fix:** place it just under the title bar, or inside the modal's
  top edge.

### ORCH-15 polish: Reviews order and wording differ from the Agents list

- **Area:** ui/src/inbox, ui/src/shell
- **Actual:** the title bar has an icon with a badge "3" (name "Reviews, 3
  waiting", tooltip "Reviews Alt+Shift+R", good for scripts); the inbox lists
  lint, tests, docs, the reverse of the Agents list (docs, tests, lint).
- **Suggested fix:** use one order (spawn order) in both places.

### ORCH-16 polish: "Jump to latest" covers text and says nothing about new content

- **Area:** ui/src/agents (Transcript.svelte)
- **Actual:** it shows as soon as he scrolls more than 60 px up, without new
  messages, and sits over the text (`orch-12-…`). Following stops correctly
  (scrollTop stayed 0 while a new turn arrived) and the button jumps to the
  end.
- **Suggested fix:** "3 new · Jump to latest" when messages arrived while
  detached; a gap under the last line so it does not cover text.

## What worked well

- The header reads at a glance: "3 of 3 · 3 queued" with three slot marks;
  the queued task says "Workers are waiting for a free agent", and its workers
  "Waiting for a free agent".
- The task row's meter ("0 of 3 done" → "3 of 3 done") is a real
  `progressbar` named "Workers done" with value and max: easy for scripts.
- Rows put "needs you" first, with Approve and Review pills; the close
  confirmation names what happens ("Close this task and its 3 workers? Their
  changes stay in Reviews.").
- "Started worker docs/tests/lint" in the coordinator's transcript says where
  workers came from; the worker's own transcript starts with "Coordinator:
  Write the docs notes."
- Tool steps are folded to one line: 9 turns made 29 entries, 10 `details`,
  0 open, 218 DOM nodes. The turn divider "Turn ended · 64.4k tokens" is quiet
  and its tooltip explains plan billing ("about $0.0024 at API prices (your
  plan's limits, not this amount)").
- Approvals are grouped per task with "No sandbox" said once (one match on the
  page), the worker named on each card, the arming fill on Allow once, Deny
  first, and Y/N keys.
- The approvals toast with a count ("3 requests need approval · Review"), the
  window title "(3) Ovra", the "3 waiting" pill and strip badge when Agents is
  folded, the auto-unfold on a new request, and the banner over modals with
  Show all keep a waiting request from being lost.
- The task-done toast is one per task: "Write the notes: 3 changes to review ·
  Review".
- Contrast of quiet text is fine: capacity and scope lines 5.9:1 and 6.7:1.
