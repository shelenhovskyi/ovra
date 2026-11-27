# 0015. A task may change several repositories

- Status: accepted (owner, 2026-11-27; details in `docs/specs/multi-repo.md`)
- Date: 2026-11-27

## Context

A task belongs to one project, and a session works in one folder: the
main checkout (ADR 0005) or one worktree. Changes that span repositories,
such as an API and its caller, need two unrelated tasks today. The owner
wants one task and one review for them.

## Decision

- A solo task ("One agent, own copy") may link up to three other projects
  of the person's project list. Its one worker gets a worktree in each,
  made like any worker's, and works in the main repository's copy with
  the others added as folders (`--add-dir`).
- The sandbox treats every linked copy as one more work dir, with the same
  protected paths. A sandbox that cannot do that refuses the profile.
- Review stays per repository: one item per repository, in its own
  project, with all of today's checks. Items of one session are shown and
  kept together. No review item spans two repositories, so fingerprints,
  partial approval, undo, pull requests and clean-up stay as they are.
- Only the main repository's agent configuration loads. Policies of a
  linked project decide file tools on paths in its copy.
- Coordinators, direct sessions, attempts and other providers do not take
  linked repositories yet.

## Consequences

- `orchestration.start`, `Task`, `Worker`, `ReviewItem` and
  `SandboxProfile` grow optional fields; nothing changes for one
  repository.
- Reviews of one project can show items of another project, when they
  belong to one session.
- A worker's write reach grows from one worktree to up to four; each is a
  worktree Ovra made for that task, never a main checkout.
