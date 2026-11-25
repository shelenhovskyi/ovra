# 0011. Research sessions: read-only agents with their own cap

- Status: accepted (owner, 2026-11-25)
- Date: 2026-11-25

## Context

Many questions need an agent that only reads: "how does the review flow
work", "which crates call the sandbox", "what do other editors do here".
Today such a question takes a direct session (which holds the project's
direct-writer lock and records a baseline) or a worker (which makes a
worktree). Both count against the cap of 3 working agents, although a
reader uses no disk and runs no builds. ADR 0005 knows only two modes,
`direct` and `isolated`.

## Decision

- A third session mode, `research`. The session runs in the project's main
  checkout, which its sandbox shows read-only (`SandboxProfile::
  work_dir_read_only`). No worktree, no baseline, no review item, no
  direct-writer lock, no dev-server ports.
- Its tools are Claude Code's `Read`, `Grep`, `Glob` and `WebSearch`
  (`--tools`). No `Write`, `Edit`, `Bash`, `WebFetch`, subagents or IDE
  tools. The core refuses any other tool a research session asks for
  (`DecidedBy::Role`), so the rule holds even where no sandbox runs.
- `WebSearch` runs on Anthropic's side; the session's network stays the
  usual netproxy allowlist. Opening web pages (`WebFetch`) is not offered:
  it would need the proxy to let the agent reach any host. A research
  session's `WebSearch` is allowed without asking unless a project policy
  rule asks or denies.
- Research sessions have their own cap per project, 2 by default, set in
  Settings (`agents.set_research_limit`, kept in `AgentSettings`). They do not
  count against the worker cap of 3, nor workers against theirs. A start
  beyond the cap is refused, like a person's direct start.
- The answer is a report in the transcript. The person may save it under
  `docs/research/` ("Save as doc", written by the UI through `fs.*`, never by
  the agent) or hand it to a new task ("Turn into task", which fills the
  task form; the person starts it).

## Consequences

- Every place that switches on `SessionMode` handles `research`; most only
  check for `direct` or `isolated` and leave it out naturally.
- The Linux sandbox binds the work dir read-only and makes nothing in it.
  The Windows sandbox grants it read access only. Without a sandbox (macOS
  today) the tool list and the core's refusal are what keep it read-only.
- Research still spends the user's subscription limits; the cap and
  Settings say so. Resource balancing counts research sessions as light.
