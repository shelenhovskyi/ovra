# Stuck agents and work cut off by a quit

Status: approved 2026-11-27. The owner picked "notice a stuck or looping
agent, and resume a task after a crash or restart" from the list of
improvements and said to take it; the details are the agent's choice.

Long agent turns fail quietly in two ways people complain about: an agent
that goes in circles (the same failing command again and again) or sits
silent, and work that is simply gone after the app closes or crashes.

## Stuck hints

The core watches each working session's current turn
(`crates/core/src/agents/stuck.rs`) and keeps `SessionInfo.stuck`; changes
go out as `agents.stuck { session, stuck }`. Three hints, first match wins:

- **Silent**: nothing from the agent (no message, no streamed text, no
  change of state) for 10 minutes while it works. A long build looks the
  same, so the words say both.
- **Repeating**: among the turn's last 12 finished tool calls, the same call
  (tool and input) got the same result (output and error flag) 3 times.
  A rerun that gets a new result is progress, not a loop.
- **Failing**: the last 5 tool calls in a row failed.

Ovra's own IDE tools (`mcp__ovra__*`, a coordinator asking about its
workers while it waits) and subagents' calls (parallel helpers may run the
same thing) do not count. A message from the person starts the count
again; a turn that ends, or any other state than working or waiting for
approval, clears the hint. The hint is never kept across restarts.

It is only a hint: Ovra never stops or nudges the agent itself.

UI, while the session works (not while it waits for approval, which needs
the person anyway): the row in the task list shows a `Stuck?` pill; the
transcript's live line gets one quiet line under it, such as "It may be
going in circles: it ran make 3 times with the same result. Stop it, or
send it a hint." Stop and the reply box are where they always are.

## Work cut off by a quit

When Ovra loads its sessions, those that still ran when it last quit or
crashed become `stopped` as before. Those that were in the middle of a turn
(`starting`, `working`, `waiting_for_approval`) and whose agent reported its
conversation id also get `SessionInfo.interrupted`. A session that had
finished its turn (`idle`) is not interrupted: it waited for the person
anyway. Any later run clears the flag.

`agents.resume_interrupted { project, session? }` continues `session`, or
every interrupted session of `project`, with Ovra's note as the next
message ("Continued after Ovra closed in the middle of this turn", then
words for the agent to check what is done before going on). Coordinators go
first, so that their resumed workers have someone to report to. Each one is
an ordinary `agents.resume`: the agent limit, accounts and cleaned-up
worktrees apply, and one that cannot continue is answered under `left` with
the reason, while the rest go on. It answers `agents.resumed { resumed,
left }`. The phone cannot call it, like `agents.resume`.

This is a person's action (a button), so agents still start only from one.

UI: over the task list, one line "2 agents stopped when Ovra closed." with
"Continue all"; the row's state reads "Stopped when Ovra closed"; an
interrupted session's detail offers "Continue" above the usual "Resume
with a message".

## Limits

- An agent process that reports its own end before Ovra's process is gone
  (a clean exit while Ovra quits) is saved as ended and is not marked
  interrupted.
- Queued and preparing sessions had no agent yet and are not resumed;
  they show as stopped.
