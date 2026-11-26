# Next tasks

Status: queue written on 2026-11-25 for parallel threads. The coordinator
starts each task as its own thread. Every task owns one area and stays out of
the areas listed under "Already running" and of the other tasks' areas.

Already running, not repeated here:

- Simplify the UI: Keep/Discard wording in Reviews, "Always allow" on
  approvals (owns `ui/src/inbox`, `ui/src/approvals`, rules in `ui/src/settings`).
- IDE P1 from `docs/specs/ide-basics.md`: multi-select, undo of file
  operations, auto save, settings, split editor (owns `ui/src/tree`,
  `ui/src/editor`, `ui/src/settings`, `crates/fs-index`).

Out of scope until the owner says otherwise: releases, code signing, the
updater, window translucency, the macOS sandbox (no Mac to test on) and the
Windows sandbox (needs a Windows machine).

## Order

| #   | Task                                        | Area                                                         | Start                                       |
| --- | ------------------------------------------- | ------------------------------------------------------------ | ------------------------------------------- |
| 1   | Orchestration gaps and the 3.E scenario     | `crates/core/src/orchestration`, `agents/roles`, `tests/e2e` | now                                         |
| 2   | Claude driver bugs from the live suite      | `crates/agent-runtime`, `tests/agents`                       | now                                         |
| 3   | Sandbox and netproxy tightening             | `crates/sandbox-linux`, `crates/netproxy`, `tests/security`  | now                                         |
| 4   | Git panel follow-ups                        | `crates/worktree/src/repo`, `ui/src/git`                     | now                                         |
| 5   | Dev server follow-ups                       | `crates/servers`, `ui/src/servers`                           | after a slot frees                          |
| 6   | Test speed and budget flakes                | `scripts/`, `.config/nextest.toml`, `crates/secrets` tests   | after a slot frees                          |
| 7   | Specs: worktree setup, checkpoints, PR flow | `docs/specs/` only                                           | any time (no code)                          |
| 8   | Review by hunk and line comments to agents  | `ui/src/inbox`, `crates/core/src/orchestration/review.rs`    | after task 1 and the Keep/Discard work land |

Tasks 1 to 4 can run at once: their areas do not overlap. A task that needs a
protocol field or a trait method adds it in its own small commit, rebased and
pushed first (CLAUDE.md, "Parallel work").

## 1. Orchestration gaps and the 3.E scenario

Why: the coordinator and its workers work, but some ends are loose, and the
plan's phase 3 gate (3.E) was never written.

- Ending the coordinator (person's stop) stops its running workers, with a
  test on fake agents.
- Live review diffs: a worker's review item updates while the worker runs,
  not only at turn end (`review.changed` from a watch on the worktree, through
  the existing `FsService` watch API; no new crate).
- Branches of cleaned-up worktrees: decide keep or delete in one place and
  document it in `docs/specs/orchestration.md`.
- 3.E: e2e on fake agents for "task, three workers, three diffs in Reviews,
  keep one, discard one, comment on one".
- Review the role prompts in `agents/roles/` against what the tools allow now.

Done when: the e2e passes under its budget and the spec matches the code.

## 2. Claude driver bugs from the live suite

Why: open bugs from 2.H (`just agents-live`) make real sessions look broken.

- An interrupted turn says "The turn failed." It should say the person
  stopped it (`result` with `error_during_execution` and `terminal_reason`
  `aborted_tools`).
- Keep `decision_reason` from `can_use_tool` and pass it to the approval
  (protocol field in its own commit; the approvals UI shows it later, in the
  simplify thread's area).
- The fake agent replays the recorded fixtures in
  `crates/agent-runtime/fixtures`, so contract tests and e2e share one source.
- Check the environment the CLI inherits against the approved allowlist.

Done when: each bug has a test on recorded fixtures; the live suite is run
once by hand if a signed-in `claude` is available, otherwise say so.

## 3. Sandbox and netproxy tightening

Why: open gaps from the attack tests (4.F) and the path review. Tightening
needs no owner question (standing rule); an independent reviewer agent
reviews it, and the report lists every change.

- Protected files that do not exist yet (`CLAUDE.md`, `CLAUDE.local.md`,
  `AGENTS.md`, `.mcp.json`, `.envrc`) cannot be created inside the sandbox.
- Names from `/etc/hosts` are checked the same way when an upstream proxy is
  set (today a documented gap).
- An attack test for each fix in `tests/security`, and the README there
  updated.

Done when: the new attack tests fail on the old code and pass on the new.

## 4. Git panel follow-ups

Why: open items from the git panel review.

- Commit and switch do not pass `--literal-pathspecs` or `-c` options on to
  the person's hooks.
- A timeout for hooks and gpg that hang, with a message and a way to cancel.
- Blame shows errors instead of hiding them behind the fallback.
- "Git: open panel" with no project open says why nothing opened.

Editor-side items (gutter markers by keyboard, whole-file delete in the
gutter) wait for the IDE P1 thread, which owns `ui/src/editor`.

## 5. Dev server follow-ups

Why: open items from the servers and browser work (ADR 0008, ADR 0009).

- Stopping a worktree waits for its pod to end.
- Create `.ovra/servers.json` from the Servers dialog for a new project.
- One overview of servers across work dirs (main checkout and workers).
- Reason codes for browser refusals from the core, shown in the pane.

`servers.open` (a forward for the person's own browser) stays out: it opens a
port to the host and needs the owner.

## 6. Test speed and budget flakes

Why: the owner wants a fast suite, and budgets fail under parallel load
(Rust wall-time budgets, `a_bound_secret_never_enters_the_sandbox` over its
1 s budget on `main`).

- Measure the Rust budget as CPU time, like the UI budget, so other agents'
  load does not fail it. Raising any budget still needs a human.
- Make the slowest five tests faster without dropping what they check.
- Keep the "five slowest" printout and record the before and after numbers in
  the commit message.

Done when: `just test` passes twice in a row with another suite running.

Follow-up (not started): the UI suite's CPU time goes mostly to mounting
whole panels. Move the logic of `AgentsPanel.svelte` (mode choice, hints,
workers under their coordinator, who wrote what) and `Inbox.svelte`
(protected files: which diffs were shown, what can be kept) into
`*.svelte.ts` models, test it there, and keep two or three component tests
per panel for the wiring. Expected saving: about 0.5 s CPU. One task per
area (agents, inbox); keep every check that Keep changes stays blocked until
each protected file's diff was shown.

## 7. Specs: worktree setup, checkpoints, PR flow

Why: three features from `research/improvement-report.md` (1.4, 1.5) and
`research/git-lsp-gaps.md` (PR flow) are larger than a day, so each starts
with a short spec the owner approves. No code in this task.

- `docs/specs/worktree-setup.md`: `.worktreeinclude`, a project setup script
  with `OVRA_WORKTREE_ROOT` and `OVRA_MAIN_ROOT`, a port per worktree, shared
  caches, disk use per worktree, safe clean-up. Runs inside the worker's
  sandbox.
- `docs/specs/checkpoints.md`: a snapshot before and after each agent turn in
  direct mode, "go back to turn N", with Bash changes included.
- `docs/specs/pr-flow.md`: open a PR and show CI status for a worker's branch
  through the person's installed `gh`, never Ovra's own login or tokens (same
  rule as `claude` in ADR 0006).

Done when: the three specs are on `main` and the owner has them to approve.

## 8. Review by hunk and line comments to agents

Why: improvement report 1.6; landing work stays serial without it.

- Keep or discard single hunks of a review item.
- Comments on lines, sent to the agent as one message.
- Starts after task 1 and the Keep/Discard wording land, because both touch
  the same review code and Reviews view.
