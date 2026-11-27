# One task across several repositories

Status: approved by the owner (2026-11-27). The owner picked the idea
("item 13" of the 2026-11-27 list: frontend and backend changed together, in
one task and one review) and approved the architecture change; this spec
says how. ADR 0018 records the decision.

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
   a project that is not a git repository, a project with agent
   configuration (`.claude`, skills, hooks) the person has not trusted as
   it is now, since the CLI may load an added folder's own skills and
   trust is decided for the main folder only, and a provider that cannot
   add folders (only Claude can, for now). `Worker.repos` (project,
   worktree, branch) tells clients. A project a task still has a copy of
   cannot be removed from the list until the task is cleaned up.
3. **Copies.** The worker gets one worktree per repository, made the way a
   worker's worktree is made today: a snapshot of that repository's main
   checkout, branch `ovra/<task>/<name>` in that repository, worktree
   `<worktrees_dir>/<project>/<task>/<name>`. If one cannot be made,
   none stay and the start fails with that repository's name.
4. **The agent.** It works in the main repository's copy and gets each
   linked copy with `--add-dir`. Its first message names every copy and
   its project's name, so it knows where `frontend` is.
5. **Sandbox.** Each linked copy is one more work dir
   (`SandboxProfile::more_work_dirs`): readable and writable, with the same
   protected paths as the main one (`.git` internals, hooks, agent
   configuration). The main checkouts stay hidden, as today. A sandbox
   that cannot protect more work dirs (macOS and Windows for now) refuses
   the profile, so the task does not start rather than start with less
   protection. Where no sandbox runs at all (for now macOS with its
   sandbox off), the session runs unsandboxed with the copies added, as
   every session there does, and says so. The netproxy allowlist is the
   main project's.
6. **Policies.** A file tool's path inside a linked copy is judged by that
   project's rules, relative to its copy. A deny or ask rule of any of
   the task's projects holds; an allow rule of a linked project allows
   only file tools whose every path is in its copy. A `Bash` call is
   allowed by the main project's rules only, as today: it runs in the main
   copy. "Always allow" is offered for the main copy only; a path outside
   every copy is handled as today. Quiet mode (ADR 0015) is off for such a
   session: its checks know only the main copy, and a command could reach
   a linked copy past that project's file rules.
7. **Review.** Each repository with changes gets its own review item, as a
   worker's item today, in its own project (`ReviewItem.project`), with its
   own diff, fingerprint, Keep, Discard, parts, checks and pull request.
   Every item of a multi-repo worker carries `ReviewItem.task_repos`, the
   task's repositories, and Reviews of any of those projects lists all of
   the session's items together under the task's name, each entry naming
   its repository, and the item says which other repositories the task
   changed. Keep moves on to the next item, so the next repository comes
   up. Only the main item counts as the agent's `request_review` and
   carries its summary; linked items show when they have changes, with
   the same summary. "Keep all" keeps every open part, the main
   repository first, as one Keep each without a commit: with the fingerprint
   listed, its own check and refusals, stopping at the first part refused
   or left with conflicts. It waits while a part's check runs, and is not
   offered while a part's check failed or a part has protected files: then
   each part is kept on its own. A comment on any of them goes to the one
   session. Checks across items of other workers compare only items of
   the same repository.
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
5. UI: the setting, "Also change", grouped Reviews.
6. Independent security review of 2 to 4, e2e with the fake agent.

## Not done (later, each its own step)

- "Plan and split" across repositories (coordinator reading every
  checkout, workers each with all copies), and "One agent, here" across
  main checkouts (one direct-writer lock and baseline per repository).
- Repositories nested in one project folder (a folder holding `frontend/`
  and `backend/` that is not a repository itself).
- Other providers, once their runtime can add folders.
- One pull request description that links the others.
- Ovra's notes and tools naming a linked repository's file as
  `<project name>/<path>`; they name absolute paths for now.
