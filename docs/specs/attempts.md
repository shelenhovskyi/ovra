# Several attempts at one task

Status: built 2026-11-26. The owner picked it from the improvement list as
optional: it is off until the person turns it on, and it must not clutter
the task form.

## Problem

Agents differ, and one agent differs from run to run. For a task that
matters, a person may want two or three tries, for example Claude and Codex,
and keep the best, instead of starting them one by one and keeping track of
which is which.

## Design

1. **Off by default.** Settings > Agents has "Offer several attempts"
   (`agentPrefs.attempts`, kept in the page's storage like "Show resource
   usage"). Only then, and only in "One agent, own copy", does the task form
   show one more picker: 1, 2 or 3 attempts. The mode hint names who tries
   it, such as "2 attempts (Claude, Codex), each on its own copy".
2. **Who tries.** The picked agent first, then every other agent that can
   start, in turn, so different agents try it before one tries twice
   (`attemptAgents`).
3. **Each attempt is a solo task.** The UI sends `orchestration.start` with
   `solo` once per attempt, the same prompt and context each time; every
   call after the first names the first task in `attempt_of`. So each
   attempt has its own worktree, branch, review item, place in the agent
   limit (past it, an attempt waits in line) and lifecycle, exactly as "One
   agent, own copy". The core refuses `attempt_of` without `solo`, one that
   names a task that is not a solo task of the same project or is itself an
   attempt, and a fourth attempt. `Task.attempt_of` tells clients which
   tasks belong together.
4. **Names.** Attempts are named "Attempt 2 (Codex) · Fix the typo" in the
   agents list and in Reviews, numbered in the order they started.
5. **Reviews.** Attempts are listed together under the task's name. An
   attempt's item says "One of 3 attempts at this task" until the person
   keeps one; after that, each of the group's items offers "Discard the
   other attempts", which discards the open items of the others and stops
   their agents. Keep moves on to the next item as shown, so the next
   attempt comes up with that button.

## Not done

- One snapshot for all attempts: each takes its own when it starts, a
  moment apart; a file saved in between is in the later ones only.
- Comparing attempts side by side, or a summary of how they differ.
- A coordinator ("Plan and split") with attempts.
