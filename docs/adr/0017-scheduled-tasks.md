# 0017. Scheduled tasks: agents that start at times the person set

- Status: accepted (owner, 2026-11-27)
- Date: 2026-11-27

## Context

ADR 0006 says agents start only from a person's action. People ask for
jobs that repeat while they are away: a nightly look for bugs, a morning
summary of what changed, a weekly check of dependencies. Other agent apps
offer this. Today the person must be at the computer and press Start each
time.

## Decision

- A **schedule** is a prompt, a kind, an agent and a time (days of the week
  and a local time of day, in the time zone the person's computer had when
  they saved it). Only the person makes, changes, pauses or removes one, in
  Settings on the computer. Agents have no tool for schedules and a paired
  phone may not use `schedules.*` (ADR 0014).
- **Off by default.** Settings > Agents has "Scheduled tasks". While it is
  off, no schedule starts anything and the Schedules pane is hidden; the
  schedules stay.
- **Two kinds only.** `research` (the default) starts a research session
  (ADR 0013): read-only, changes nothing. `own_copy` starts one agent in its
  own worktree (a solo task): its changes wait in Reviews, where only the
  person keeps them. A schedule never starts a direct session (it would
  change the main checkout unwatched) or a coordinator.
- **Same limits as a person's start.** A research run takes a research
  place and an own-copy run waits in line past the agent limit, exactly as
  the person's own start would. Approvals, policies, the sandbox and the
  netproxy are unchanged: a run that needs an approval waits for the person.
- **No pile-up.** A run is skipped while the schedule's last run is still
  working, or while an own-copy run's changes still wait in Reviews.
- **Only while Ovra runs.** Ovra does not install a system timer or keep the
  computer awake for a schedule. A run may start up to an hour late (the
  computer slept, the research places were full); later than that it is
  recorded as missed, and Ovra never starts missed runs on launch.
- The prompt is the person's own text, written when they saved the
  schedule, and is sent as theirs. Sessions a schedule started carry its
  name (`SessionInfo::schedule`) so lists say "Scheduled".

## Consequences

- ADR 0006's rule reads: agents start from a person's action, or from a
  schedule the person made and turned on.
- A scheduled run spends the user's plan limits without the person watching;
  Settings says so next to the switch.
- The core needs local time: schedules keep an IANA time-zone name and the
  core computes the next run with a bundled time-zone database (`jiff`), so
  it reads nothing from the OS for it.
- Runs are not caught up after the computer was off; people who need that
  use a server's own scheduler.
