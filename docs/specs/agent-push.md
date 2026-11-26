# Agents push with the person's permission

Status: asked for by the owner on 2026-11-26 ("an agent should be able to
push with the person's permission, or push without asking once the person
allowed it"). It loosens ADR 0010's "agents get no tool for pushing", so
ADR 0010 is amended with it. Builds on `docs/specs/pr-flow.md`.

## Problem

A worker that finished its task can only ask for review. Publishing its
branch so CI runs, or so a teammate sees it, takes the person through the
review dialog every time, even for a branch they let agents use freely.

## Goals

- A worker calls the IDE tool `push`. Ovra builds one commit of the
  worker's changes, as the person, and pushes it to a branch on a remote
  with the person's own `git`, outside the sandbox.
- The person approves each push on an approval card that shows the
  remote and its host, the branch, whether it is new, what the commit
  builds on, the files with line counts, and the message.
- "Always allow" on that card adds a project rule, so later pushes to the
  same branches go out without asking. Rules are listed and removed in
  Settings > Rules like every other one.

Not goals for v1: direct sessions (step AP.4, below), pushing a branch's
history commit by commit, force pushes, deleting remote branches, opening
pull requests from the tool (the person's review dialog does that).

## What is pushed

Agents never commit: a worker's git folder is read-only
(`docs/specs/security.md`). So `push` works like the pull request flow:

1. Snapshot the worker's worktree. Refuse while it holds the sandbox's
   placeholders (as `pr.create` does).
2. Refuse when any file the review marks as protected changed
   (`.github/workflows/`, `.husky/`, agent configuration, ...): pushing a
   workflow runs it on GitHub with the repository's secrets. The agent is
   told to ask the person to publish through review, where the dialog has
   the explicit tick.
3. Fetch the branch from the remote. If it exists, the commit builds on
   its head and the push is a fast-forward with a lease on that head. If
   it does not, fetch the base (`base`, default the branch the main
   checkout's branch tracks, else `main`) and build on that; the push then
   needs the branch to be missing still.
4. Pick the worker's changes (from its start snapshot, or from the snapshot
   of its own last push or the person's last push when the branch is still
   there, to the new snapshot)
   onto that commit with `git merge-tree` and commit the tree as the
   person, without hooks. A conflict, or nothing left to push, is
   answered to the agent with the files and what to do.
5. Ask, or apply a rule (below). On allow, `Forge::push` sends it: never
   `--force`, `--no-verify`, lease on what step 3 saw. If the remote moved
   meanwhile, the agent is told to call `push` again.

Every push is a `branch_pushed` line in the audit log, and the decision is
the usual tool line. When the branch is the one of the worker's pull
request, the item's pull request follows the push, so the person's next
Update does not fail on the lease.

## The tool

`push` (IDE tool, workers only; a coordinator has no changes of its own):

| Input     | Meaning                                                                  |
| --------- | ------------------------------------------------------------------------ |
| `message` | Required. Commit message: a title line of at most 72 characters, a body. |
| `branch`  | Optional. Default the worker's own branch, `ovra/<task>/<name>`.         |
| `remote`  | Optional, a remote's name. Default `origin`.                             |
| `base`    | Optional. What a new branch builds on.                                   |

It answers when the push is done or refused, so the agent waits as for any
approval. Names are checked like the pull request dialog's (`ref_name`,
`remote_name`): never a URL, an option or a path.

## Rules

Rules for pushes use the pseudo-tool `Push` in the project's policy, with a
match `<remote> <branch>`, where the branch may end in `/*` for every
branch below it: `Push(origin ovra/*)`, `Push(origin feature-x)`. Without a
match a rule covers every push. As for other tools, `deny` wins, then
`ask`, then `auto`, and `auto` counts only in a sandboxed session whose
network is limited (every worker on Linux and macOS).

- **Protected branches** always ask unless a rule names them exactly: a
  rule ending in `/*` or without a match never pushes to them without
  asking. Protected are `main`, `master`, `trunk`, `develop`, `development`,
  `production`, `prod`, `stable`, `release`, `staging`, `next`,
  `gh-pages`, `release/*`, `release-*`, `hotfix/*`, and the branch the
  main checkout's branch tracks on that remote. (The remote's default
  branch by `refs/remotes/<remote>/HEAD` is a follow-up: it needs a forge
  call.)
- **Other branches than the worker's own** are treated like protected
  ones: a wide rule pushes only to the worker's own `ovra/<task>/<name>`.
  Another live worker's branch, or the branch of another item's pull
  request, is refused outright. "Always allow" is never offered for them; only the person can
  write such a rule, in Settings.
- **The item's own pull request branch** counts as protected too: people
  may have read or approved the pull request, or turned on auto-merge, so
  a wide rule never pushes new code into it without a card.
- **What "Always allow" offers**: `Push(<remote> ovra/*)`, which lets
  each worker push to its own branch. Never for a protected branch or
  another branch. The card says it in words: "Always allow pushes to ovra/… on
  origin".
- **Force push** does not exist for agents, with or without a rule.
- An agent tool that happens to be named `Push` is never decided by these
  rules.

When a worker pushes to the branch of its own pull request, the pull
request's lease follows (`pushed`), but what the person reviewed
(`published_from`) does not: the item still shows those changes as not
reviewed.

## Phone

The phone companion shows push cards and may Allow or Deny them once, like
other approvals; "Always allow" stays on the computer (ADR 0014).

## Steps

- AP.0 Spec and ADR 0010 amendment (this file).
- AP.1 `Push` rules in the policy: check, decide, offered rule; protocol
  docs on both sides.
- AP.2 `push` in `tools.json`, the MCP handler and the core: plan, ask,
  push, audit, pull request follow-up. Fakes and tests.
- AP.3 The approval card for `Push`, Settings > Rules shows push rules in
  words.
- AP.4 Direct sessions: they have no IDE tools yet, and their changes are
  mixed with the person's own in the main checkout; needs a pick of only
  the agent's files. Later, with its own short note here.

## Security

This loosens ADR 0010 on the owner's request. What keeps it narrow:

- The agent never gets credentials: Ovra runs the push outside the sandbox
  with the person's `git`, as for pull requests, and the sandbox still
  hides `gh`'s configuration and the token variables.
- What goes out is one commit built by Ovra from the worker's files, shown
  on the card; hooks never run; workflows and other protected files never
  go out through the tool. The message may not carry `Reviewed-by` or
  `Signed-off-by` trailers, control or text-direction characters.
- Only remotes whose URL names a host: a remote that is a folder on this
  computer is refused.
- **What this does not stop**: the pushed code (tests, build scripts,
  `package.json` scripts) runs in the repository's CI as soon as the branch
  is pushed, with the secrets and network CI has there, outside Ovra's
  sandbox and netproxy. The card says so on every push, and that is the
  reason a rule covers only workers' own branches.
- No force, no deletes, protected branches ask unless named exactly.
- The tool, the policy and the core code are security code and need an
  independent reviewer agent's verdict.
