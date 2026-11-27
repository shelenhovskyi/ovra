# Check before review, and starts that wait for a place

Status: both features approved by the owner on 2026-11-25 (from the review
of an agent's ideas); the details below are the agent's, within that
approval. "Honest done" (person's own sessions, saved results, weakened
checks) added 2026-11-27, approved with the owner's improvement list. Builds on `docs/specs/orchestration.md`,
`docs/specs/worktree-setup.md` and `docs/specs/agent-resources.md`.

## 1. Check before review

### Problem

A worker says "done" and asks for review. The person opens the diff
without knowing whether the project still builds and its tests pass, and
finds out only after Keep.

### Behaviour

- The person defines one check in `.ovra/check.json` of the main checkout:

  ```json
  { "command": ["just", "check"], "timeout": 600 }
  ```

  A program and its arguments, without a shell; `timeout` in seconds,
  default 600, at most 3600. `.ovra` is protected, so only the person
  changes it, and the file is read from the main checkout, never from the
  worker's worktree. Without the file nothing runs and nothing shows. A
  broken file shows as a failed check whose log says what is wrong.
- When a worker asks for review (`request_review`, its own or its
  coordinator's), Ovra runs the check once in the worker's worktree, as
  soon as the worker's turn ends, so it judges the files the worker
  finished with. A new request starts it again; the earlier run is
  killed. An unfinished run is also killed when the review is decided
  (before Keep or Discard touches files) or the worker ends.
- **Where it runs.** Exactly where worktree setup runs: the pod of the
  worktree (`servers::run_steps`, `DevServers::setup` with one step), the
  agent's sandbox without the agent's state, with the project's package
  caches and a netproxy with the project's allowlist. The check runs the
  worker's code, so it gets no more than the worker has. The same
  environment as a setup step.
- **A person's own agents.** A session the person started (one agent here,
  in the main checkout, or one agent in its own copy) has nobody to ask for
  review: its check runs when a turn ends with changes by the agent that no
  check has judged yet. A direct session's check runs in the main
  checkout's pod, where its dev servers run. Workers of a task with a
  coordinator still run it only when review is asked for.
- **Stale results.** When the agent works again (a comment, a message), an
  unfinished check is killed. A finished result stays with the item, with
  the look of the files it judged; once the files differ it shows as
  "on earlier files", and the person can press Run again
  (`review.run_check`), as for a failed check. A turn that changes nothing
  keeps the result.
- The run does not take one of the agent places: it is short, and the
  worker it belongs to is idle while it runs.
- Its output comes from code an agent wrote: it is shown as plain text
  with terminal escapes, control characters and direction overrides
  removed, and never sent to an agent.
- A finished result is saved with the review item and survives a restart;
  its log stays in memory only and keeps the last 2000 lines (each cut at
  2000 characters).

### What the person sees

- Reviews, on the item: one quiet line under the summary, "Check running",
  "Check passed" or "Check failed" (with "timed out" or the exit code),
  never a toast or notification. "Show log" opens the output as plain
  text. Nothing when the project has no check.
- The list row gets no marker: the state line is enough.

### Weakened checks

Agents that cannot make a test pass sometimes edit the test, skip it or
silence the check, then say they are done. Ovra reads the diff, not the
agent's words. `review.warnings { id }` lists what the agent's own changes
do to tests and checks: a deleted test file, edited lines of an existing
test, removed assertions (more removed than added), skipped or `only`
tests, silenced lint, type or compiler checks (`eslint-disable`,
`@ts-ignore`, `#[allow(`, `# noqa`, ...), and changed check config (CI
workflows, `justfile`, lint, type and test runner config). Test files are
found by the usual names. At most three line warnings per file and kind.
Reviews shows them as a quiet "Look closer" list under the check line; a
press opens that file's diff. Nothing blocks Keep: changing a test on
purpose is normal work. The lines come from the agent and are shown as
plain text, cut at 160 characters, without hidden characters.

### Protocol

- `ReviewItem::check: Option<ReviewCheck>`: `state` (`running`, `passed`,
  `failed`, `not_run`), `command` (as shown), `detail` (why it failed, in
  words), `stale` (it judged files that changed since).
- `review.check_log { id }` answers `review.check_log { lines }`.
- `review.run_check { id }` runs it again for an open item; answers `ok`.
- `review.warnings { id }` answers `review.warnings { warnings }`.
- `review.changed` is sent when a check starts and ends.

### Not now

- Telling the coordinator the result, so it can send the worker back.
  Useful, but it puts command output in front of an agent; a follow-up
  with its own review.

## 2. Starts that wait for a place

### Problem

At the limit (Settings, 1 to 8, default 3), a person's "One agent, here"
start is refused, and they have to come back later and type it again.

### Behaviour

- A person's start past the limit waits `queued`, as workers already do,
  and starts on its own when a place frees, oldest first, workers and
  people's starts in one line. Starting it was still the person's action
  (ADR 0006): Ovra only delays it. The shortage gate of
  `docs/specs/agent-resources.md` applies to the whole line.
- A waiting direct session takes the main checkout's writer lock and
  baseline when it is queued, so two waiting direct starts cannot both
  run later; the second is refused as today.
- `AgentCapacity::waiting`: the waiting sessions of the project, in the
  order they start, so the UI shows a position.
- Cancel is Stop on a waiting session: it never starts. Messages to it
  are refused, as for a waiting worker.

### What the person sees

- Start stays enabled at the limit; the hint under the box says "All 3
  agents are busy. This starts when one finishes."
- The row says "Waiting, 2nd in line"; its menu offers "Cancel".

## Tests

Fakes only: queued start of a person's session and its start when a place
frees, its position, cancel; check runs on request with the fake
`DevServers`, passes, fails, times out, broken file, no file, dropped when
the worker works again, log. UI unit tests for the line and the log.
