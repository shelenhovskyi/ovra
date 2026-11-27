# 0015. Quiet mode: contained actions run without asking

- Status: accepted (owner, 2026-11-27)
- Date: 2026-11-27

## Context

Every tool call an agent makes asks the person unless a project rule
decides it (`docs/specs/security.md`, "Policies"). People approve almost
all of these questions, learn to click Allow without reading, and then
turn on tools that skip every question. Ovra already runs agents in a
sandbox whose only way out is the netproxy allowlist, so most of what an
agent does could not hurt anything outside its own folder even if it were
a mistake. The owner approved a mode that stops asking about such calls
(2026-11-27), as a security loosening the person turns on.

## Decision

- Each project's policy gets a `quiet` switch, off by default, set in
  Settings > Rules. Only the person sets it, like every other policy
  change.
- With it on, a call that no project rule decides is allowed without a
  question when the sandbox would contain it: the session runs in a
  sandbox Ovra trusts for this (today only the Linux one), with the
  network limited, and the call is one of the kinds the spec lists
  (reads and searches inside the folder; in a worktree, edits outside
  protected paths and commands that change no git state; web requests to
  named hosts). Everything else still asks.
- Deny and ask rules still win, and a file rule holds for every tool
  that names its path (a `Read(.env)` deny keeps `cat .env` asking). Keys
  still ask on first use, Ovra's own
  pushes keep their rules, and the decision is written to the audit log
  like any policy decision.
- Where the sandbox is not verified (macOS until it is checked on a Mac,
  Windows until its network gaps close) or reports an open network,
  quiet mode changes nothing.

## Consequences

- A prompt injection that stays inside the sandbox now runs without a
  person seeing it first; its changes still wait in Reviews. Data can
  leave to any host the allowlist lets through, broad entries included,
  as for any sandboxed command the person allowed.
- Edits and commands in a direct session still ask: they land in the
  person's own checkout, where their own tools may run an edit before any
  review and Discard does not undo a command.
- A platform sandbox joins the trusted list in its own change, after it is
  verified.
