# Plan first, and replies that wait

Status: built 2026-11-27. The owner picked both from the improvement list
(items "plan first" and "message queue while the agent works"). Plan first is
optional: it is off until the person turns it on in Settings > Agents.

## Problem

1. People want to see what an agent means to do before it touches their
   files, and say "go" or "not like that". Today the first thing they see is
   the change itself, in an approval card or in Reviews.
2. While an agent works, the reply box is disabled. A person who thinks of
   one more thing must wait for the turn, or Stop it and lose the step.

## Plan first

1. **Off by default.** Settings > Agents has "Offer "Plan first""
   (`agentPrefs.planFirst`, kept in the page's storage like the other
   choices there). Only then, and only for "Work in this folder" and "One
   agent, own copy", does the task form show a "Plan first" box, checked.
   It is not offered for a provider without an enforced policy: Ovra could
   not hold its changes.
2. **Start.** `agents.start` (direct) and `orchestration.start` (with
   `solo`) take `plan_first`. The core refuses it for any other session
   (coordinators, research, a task split among workers) with `bad_request`,
   and for a provider without `enforced_policy` with `unavailable`. The first
   message gets a note in Ovra's words after the person's prompt: look
   around, reply with a short numbered plan, end the turn.
3. **Ovra holds the line, not the agent.** While `SessionInfo::planning` is
   set, the core refuses every `edit`, `delete` and `move` call in both the
   `PreToolUse` hook and `can_use_tool`, and puts every `execute` call to the
   person: the hook answers `ask`, an `auto` rule does not apply, and the
   card offers no "Always allow". Its reason says the agent wants it before
   the plan was approved. Reads, searches, web fetches (still under the
   project's policy and netproxy) and Ovra's own IDE tools go on as usual.
   Subagents' calls come through the same session, so they are held too.
4. **The plan.** The agent's turn ends with its plan as an ordinary message.
   The transcript shows a line above the reply box: "Planning. The agent
   changes no files until you approve its plan." while it works, and "Plan
   ready. Approve it to let the agent change files, or reply to change the
   plan." with an **Approve plan** button once its turn ended. A reply keeps
   the session planning; the agent revises.
5. **Approve.** `agents.approve_plan` clears `planning` (event
   `agents.planning`), and sends the agent Ovra's "The person approved your
   plan. Carry it out now; you may change files." Refused (`conflict`) while
   a turn runs or for a session that does not plan. A phone cannot approve a
   plan: it lets the agent change files, like Keep.
6. **Kept.** `planning` is part of the saved session: a session resumed
   after a restart still plans until approved.

Not built: Claude Code's own plan mode (`--permission-mode plan`). It writes
the plan to a file under the account's folder, outside the project, which
Ovra's rules would ask about at every write, and it depends on the CLI's
internal ExitPlanMode flow. Ovra's own hold works for every provider with an
enforced policy and is enforced by Ovra, not by the agent.

## Replies that wait

1. **Queue.** While a session works, waits for an approval, or waits to
   start, the reply box stays open; its button says **Queue**.
   `agents.queue` keeps the message (with its `@` context, read when it was
   queued) in the session's queue (`SessionInfo::queue`, event
   `agents.queue`); for an idle session it is a plain send. At most
   `MAX_QUEUED` (10) wait. Elements picked in the session's browser go with
   the next message the person sends, not with a queued one.
2. **Delivery.** When a turn ends (the session goes `idle`), the core sends
   the oldest queued message as the next turn. So messages go one by one,
   each in a turn of its own, and nothing interrupts the agent.
3. **Change your mind.** The queue shows above the reply box with **Edit**
   (back into the box, out of the queue) and **Remove** (`agents.unqueue`;
   `not_found` once it was sent), and **Send now**, which stops the current
   step (`agents.interrupt`) so the first queued message goes at once.
4. **Ends with the session.** A session that stops or fails drops its
   queue. The queue is not saved across restarts.

## Tests

Core: `agents/tests/plan_queue.rs` (queue order, limits, unqueue, idle send,
stop; plan first refusals, asks, no Always allow, approve), solo task tests in
`orchestration/tests/solo.rs`. UI: `AgentsPanel.plan-queue.test.ts`,
`prefs.test.ts`. e2e: the plan-first test in `agents.spec.ts`.
