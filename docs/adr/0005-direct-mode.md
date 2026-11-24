# 0005. Direct mode: one agent may work in the main checkout

- Status: accepted
- Date: 2026-11-24

## Context

Worktrees isolate parallel agents, but for a single task they add friction:
the person cannot watch files change in their editor, builds and caches are
duplicated, and the result must be merged back. The owner wants a mode where
one agent simply works in the project's main checkout.

## Decision

- Each agent session runs in one of two modes: `isolated` (own git worktree,
  the default for subagents) or `direct` (the main checkout).
- At most one direct session per project at a time (the "direct writer"
  lock, held by the core). A second direct request is refused; the caller may
  start it isolated instead.
- When a direct session starts, the core records a baseline snapshot of the
  working tree, including uncommitted and untracked files, as a git tree
  object written through a temporary index. HEAD, the real index and the
  stash are not touched.
- The session's diff for review is baseline -> current working tree. Hooks
  report which files the agent wrote; files changed in the diff that the
  agent did not write (the person editing, a formatter, git) are shown in the
  inbox as "changed outside the agent" and are never reverted automatically.
- Approve keeps the changes and optionally commits the agent's files with a
  drafted message. Reject restores the agent's files from the baseline,
  except files changed outside the agent since, which become a conflict for
  the person to resolve. A comment resumes the same session.
- Isolated agents started while a direct session is active branch from a
  snapshot commit of the current working tree, so they see work in progress.
  Their approved changes land in the main checkout only for files the direct
  session has not touched; overlapping files wait until the direct session is
  approved or rejected.

## Consequences

- Phase 2's single agent already works this way; phase 3 adds the baseline,
  the diff, revert and the lock.
- From phase 4 the direct agent's sandbox may write the whole project root;
  `.git` stays writable for commits, while push and history rewrites remain
  behind approval.
- People editing the same files at the same time as the agent is possible but
  not merged automatically; the editor's change banner and the inbox's
  "changed outside the agent" marker make it visible.
