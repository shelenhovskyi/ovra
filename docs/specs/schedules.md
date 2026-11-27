# Scheduled tasks

Status: approved by the owner on 2026-11-27 (project chat, "all useful,
take them"), as opt-in. Decision record: ADR 0015.

## Problem

Some jobs repeat: look for bugs every night, summarize yesterday's changes
every morning. Today the person must start each run themselves.

## What the person sees

- Settings > Agents: a switch **Scheduled tasks**, off. Its hint: "Agents
  start by themselves at the times you set, only while Ovra is open. Each
  run uses your plan like a task you start."
- With the switch on, the project's Settings get a **Schedules** pane.
  Empty, it says what a schedule is and has **Add schedule**.
- The form: Name; What to do (the prompt); Kind: **Research** ("Reads and
  reports, changes nothing", the default) or **One agent, own copy**
  ("Changes wait in Reviews"); Agent (only when more than one can start);
  Days (Mon to Sun, every day by default) and Time (24-hour `HH:MM`).
- Each schedule's row: name, "Every day at 03:00" or "Mon, Wed at 09:30",
  the next run, the last run's outcome ("Started", "Skipped: the last run's
  changes still wait in Reviews", "Missed: Ovra was not running"), and
  **Run now**, **Pause**/**Resume**, **Edit**, **Remove**.
- A scheduled session's row in the agents list and History says
  "Scheduled · <name>".

## Core

- Protocol domain `schedules.*` (`protocol/schedules.rs`):
  - `schedules.enabled` / `schedules.set_enabled { enabled }` answer
    `schedules.enabled`; setting sends `schedules.changed { project: null }`.
  - `schedules.list { project }` answers the project's schedules, oldest
    first, each with `next_ms` (`None` while paused or switched off).
  - `schedules.save { project, id?, draft }` makes or replaces one and
    answers it; the draft is checked (name and prompt not empty, prompt
    within the usual size, at least one day, `HH:MM`, a known time zone).
  - `schedules.set_paused { id, paused }`, `schedules.remove { id }`,
    `schedules.run_now { id }` (the person's action: runs even while the
    switch is off or the schedule is paused, with the same skip rules).
  - Event `schedules.changed { project }` after any change or run.
- Kept in `AgentSettings` (`schedules_enabled`, `schedules`), so the store
  needs no change.
- The clock: one task wakes every 30 seconds (tokio time, so tests pause it)
  and fires every schedule whose due time has come and is at most an hour
  old. A due time older than that is recorded as missed and the next one
  computed. Starting a run that cannot get a research place leaves it due,
  so it is tried again on the next tick within the hour.
- Firing reuses the person's paths: `agents.start` with `research` mode, or
  `orchestration.start` with `solo`, with the schedule's agent and prompt.
  The new session gets `SessionInfo::schedule` (the schedule's name).
- Skips: the last run's session is starting, preparing, queued, working or
  waiting for approval; or, for an own-copy run, its review item is open.

## Tests

- Next run: every day, chosen days, a time already passed today, a DST
  change (the local time stays), a bad time zone refused.
- Firing: a due schedule starts a research session marked with its name;
  switched off or paused fires nothing; an old due time is missed and not
  started; a full research cap leaves it due, then skipped after the hour;
  a working last run skips; an own-copy run with an open review skips.
- Requests: drafts are checked; a phone may not use `schedules.*`.
- UI: the switch shows the pane; the form saves a schedule; Run now.
- e2e: turn the switch on, add a research schedule, Run now, and see
  "Scheduled" on the fake agent's session.

## Not done

- Runs while Ovra is closed or the computer sleeps.
- Other repeat rules (every N hours, monthly).
- A coordinator ("Plan and split") or a direct session on a schedule.
- `@` context in a scheduled prompt.
