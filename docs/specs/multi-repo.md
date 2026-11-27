# One task across several repositories

Status: approved by the owner (2026-11-27). The owner picked the idea
("item 13" of the 2026-11-27 list: frontend and backend changed together, in
one task and one review) and approved the architecture change; this spec
says how. ADR 0015 records the decision.

## Problem

A change often spans repositories: an API in `backend` and its caller in
`frontend`, a library and the app that uses it. Today a task works in one
project, so the person runs two agents that do not see each other's work,
and reviews and lands two unrelated results.

## Words

- **Main repository**: the project the task starts in. The agent's working
  folder is its copy, and only its own agent configuration loads.
- **Linked repository**: another project from Ovra's project list that the
  person adds to the task. Each must be a git repository.

## Design

1. **Off by default.** Settings > Agents gets "Tasks across several
   repositories" (`agentPrefs.multiRepo`, kept like "Offer several
   attempts"). Only then, and only in "One agent, own copy", does the task
   form show "Also change" with a picker of the person's other git projects
   (at most 3 linked, 4 repositories in all). Chips name each picked one;
   removing a chip removes it.
2. **Protocol.** `orchestration.start` with `solo` takes `repos`: the ids of
   the linked projects. The core refuses `repos` without `solo`, with
   `attempt_of`, the task's own project, a project twice, more than three,
   a project that is not a git repository, and a provider that cannot add
   folders (only Claude can, for now). `Task.repos` and `Worker.repos`
   (project, worktree, branch) tell clients.
3. **Copies.** The worker gets one worktree per repository, made the way a
   worker's worktree is made today: a snapshot of that repository's main
   checkout, branch `ovra/<task>/<name>` in that repository, worktree
   `<worktrees_dir>/<project>/<task>/<name>`. If one cannot be made,
   none stay and the start fails with that repository's name.
4. **The agent.** It works in the main repository's copy and gets each
   linked copy with `--add-dir`. Its first message names every copy and
   its project's name, so it knows where `frontend` is. Ovra's notes and
   tools that name files name a linked repository's file as
   `<project name>/<path>`.
5. **Sandbox.** Each linked copy is one more work dir
   (`SandboxProfile::more_work_dirs`): readable and writable, with the same
   protected paths as the main one (`.git` internals, hooks, agent
   configuration). The main checkouts stay hidden, as today. A sandbox
   that cannot protect more work dirs (macOS and Windows for now) refuses
   the profile, so the task does not start rather than start with less
   protection. The netproxy allowlist is the main project's.
6. **Policies.** A file tool's path inside a linked copy is judged by that
   project's rules, relative to its copy, and "Always allow" on it adds the
   rule to that project. A `Bash` call is judged by the main project's
   rules, as today: it runs in the main copy. A path outside every copy is
   handled as today.
7. **Review.** Each repository with changes gets its own review item, as a
   worker's item today, in its own project (`ReviewItem.project`), with its
   own diff, fingerprint, Keep, Discard, parts, checks and pull request.
   Every item of a multi-repo worker carries `ReviewItem.task_repos`, the
   task's repositories, and Reviews of any of those projects lists all of
   the session's items together under the task's name: "Changes in 2
   repositories". Above them, "Keep all" keeps each open item in turn and
   stops at the first one that is refused or conflicts, saying which. A
   comment on any of them goes to the one session.
8. **Lifecycle.** Stop, resume and clean-up treat the linked copies as
   they treat the worker's worktree: once the task ended and every item of
   the worker is settled, each copy is removed and its branch deleted
   unless it has other commits; a copy git will not remove keeps the
   worker's worktree too, and the task says why. A copy is made only with
   enough free disk space; otherwise the start is refused. Worktree setup
   (`docs/specs/worktree-setup.md`), the review check
   (`.ovra/check.json`), the agent's own `push`, the live diff while it
   works and dev servers cover the main copy only; a linked item's diff
   updates when a turn ends.

## Phases

1. Protocol fields and the profile field (shared hot spots, own commits).
2. Core: `repos` on solo starts, worktrees, `--add-dir`, first message,
   sandbox profile, refusal on macOS and Windows sandboxes, clean-up.
3. Review: an item per repository, `task_repos`, listing across projects.
4. Policies per linked repository.
5. UI: the setting, "Also change", grouped Reviews with "Keep all".
6. Independent security review of 2 to 4, e2e with the fake agent.

## Not done (later, each its own step)

- "Plan and split" across repositories (coordinator reading every
  checkout, workers each with all copies), and "One agent, here" across
  main checkouts (one direct-writer lock and baseline per repository).
- Repositories nested in one project folder (a folder holding `frontend/`
  and `backend/` that is not a repository itself).
- Other providers, once their runtime can add folders.
- One pull request description that links the others.
