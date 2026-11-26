# Check before review, and starts that wait for a place

Status: both features approved by the owner on 2026-11-25 (from the review
of an agent's ideas); the details below are the agent's, within that
approval. Builds on `docs/specs/orchestration.md`,
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
  coordinator's), Ovra runs the check once in the worker's worktree. A
  new request starts it again; the earlier run is killed.
- **Where it runs.** Exactly where worktree setup runs: the pod of the
  worktree (`servers::run_steps`, `DevServers::setup` with one step), the
  agent's sandbox without the agent's state, with the project's package
  caches and a netproxy with the project's allowlist. The check runs the
  worker's code, so it gets no more than the worker has. The same
  environment as a setup step.
- **Stale results.** When the worker works again (a comment, a message),
  its check is dropped (a running one is killed): the files it judged are
  gone. The next request runs it again.
- The run does not take one of the agent places: it is short, and the
  worker it belongs to is idle by then.
- Results are kept in memory with the review item, not across restarts.
  The log keeps the last 2000 lines (each cut at 2000 characters).

### What the person sees

- Reviews, on the item: one quiet line under the summary, "Check running",
  "Check passed" or "Check failed" (with "timed out" or the exit code),
  never a toast or notification. "Show log" opens the output as plain
  text. Nothing when the project has no check.
- The list row gets no marker: the state line is enough.

### Protocol

- `ReviewItem::check: Option<ReviewCheck>`: `state` (`running`, `passed`,
  `failed`), `command` (as shown), `detail` (why it failed, in words).
- `review.check_log { id }` answers `review.check_log { lines }`.
- `review.changed` is sent when a check starts and ends.

### Not now

- Telling the coordinator the result, so it can send the worker back.
  Useful, but it puts command output in front of an agent; a follow-up
  with its own review.
- Checks for direct sessions (the person's own checkout).

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
