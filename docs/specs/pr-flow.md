# Pull requests through the person's `gh`

Status: approved by the owner on 2026-11-25 with the recommended option of
every decision below; task 7 of `docs/roadmap-next.md`.
The crate boundary and the credentials rule are ADR 0010.
Research: `research/git-lsp-gaps.md` (PR flow) in the project files.

## Problem

A worker's result can only be kept in the main checkout or discarded.
Teams that land work through pull requests copy it by hand: keep, make a
branch, commit, push, open the PR in a browser, then watch CI there.

## Goals

- From a worker's review item: **Open pull request**. Ovra makes a commit
  of exactly what the person reviewed, on top of the remote's base branch,
  pushes it and opens a PR with the person's installed `gh`.
- The item then shows the PR and its checks, and a failed check's log can
  be sent to the worker as a comment.
- Ovra never has a GitHub login of its own and never touches the person's.

Not goals for v1: merging from Ovra (the person merges on GitHub), PRs
from forks, other forges (GitLab through `glab` could sit behind the same
trait later), push or PR from the git panel for the person's own branches,
PR review comments in Ovra.

## The credentials rule

The same as for `claude` (ADR 0006), so the person's account stays theirs:

- Ovra runs the person's installed `gh` and `git` unmodified, found the way
  the runtime finds `claude`. It never bundles, downloads or installs `gh`,
  and has no GitHub sign-in UI: when `gh` is missing or signed out, the
  item says "Install GitHub CLI (`gh`) and run `gh auth login` in a
  terminal", with a button that opens one.
- Ovra never reads, copies or passes on GitHub credentials
  (`~/.config/gh/hosts.yml`, the keychain, `GH_TOKEN`, `GITHUB_TOKEN`, git
  credential helpers' stores) and never calls GitHub's API itself. These
  processes get the person's environment unchanged, as `claude` does
  outside a sandbox; `gh` and git's credential helper find the login.
- Sign-in state comes from `gh auth status --active --hostname <host>
  --json hosts`, reading only the login and state fields. `--show-token`
  is never passed.
- Only a person's click starts a push or a PR, after a dialog that shows
  what will be sent (below). Agents get no tool for it, and in the sandbox
  `gh` finds no login: `~/.config/gh` is hidden and token variables are not
  on the allowlist (`docs/specs/security.md`). `tools.json` is unchanged.
- Remote URLs can hold a token (`https://user:token@github.com/...`).
  Ovra passes remotes by name, and removes the user part of any URL in
  output it shows or logs.
- Every push and PR is a line in the audit log.

## What is pushed

The worker's own branch is not pushed: it starts at a snapshot of the main
checkout, which holds the person's uncommitted work and Ovra's snapshot
author. Instead:

1. Fetch the base branch from the remote (`git fetch <remote> <base>`,
   default `origin` and the remote's default branch; the dialog can change
   both).
2. Pick the worker's changes onto it without a working tree: the reviewed
   snapshot (the one the item's fingerprint names) as "theirs", the
   worker's start snapshot as the merge base, the fetched base as "ours"
   (`git merge-tree --write-tree --merge-base`, git 2.40 or later; an
   older git is refused with "Pull requests need git 2.40 or later"). The
   person's
   uncommitted work is in both the start snapshot and the reviewed one, so
   it drops out; only the worker's changes remain.
3. A conflict stops here and names the files. The item offers "Ask the
   worker to update", which sends the worker a comment asking it to merge
   the base branch in its worktree; the person reviews the result again.
4. Commit the tree with the fetched base as parent, as the person (their
   `user.name` and `user.email`), with the reviewed title and body as the
   message. No hooks run: they could run project code the agent wrote,
   outside the sandbox; CI checks the PR instead. If the person's config
   sets `commit.gpgsign`, the commit is signed with their key through git.
5. Push that commit to `refs/heads/<branch>` on the remote, default
   `ovra/<task>/<name>`, editable in the dialog. Never `--force`. Pushes
   run with `--no-verify` for the same reason as hooks above.
6. `gh pr create --base <base> --head <branch> --title=<title>
   --body-file -` (body on stdin, so no text becomes an option), as a
   draft by default. `GH_PROMPT_DISABLED=1` and `GH_NO_UPDATE_NOTIFIER=1`
   are added so it never waits for input.

**The dialog** shows the remote and its host, base and branch names, the
files and line counts, and the title and body. The worker drafts title
and body; the person edits them. That click is the approval.

**Updating.** After more review rounds the item offers **Update pull
request**: the same pick, with the PR branch's head as parent, pushed as a
fast-forward. If the remote branch moved (someone pushed to it), Ovra
stops and says so instead of overwriting.

## The review item

- New state `published`, beside approved and rejected: the work lives on
  the remote. The item keeps its PR (`number`, `url`) and shows a chip:
  "PR #123 · draft · 3 of 5 checks passed". A comment still resumes the
  worker and reopens the item, which then offers Update.
- The worktree is cleaned up like a decided one once the task ends; the
  local branch follows the existing branch rule. The remote branch is the
  person's to delete (GitHub does it on merge when the repository says so).
- Keep and Discard stay available for a published item, so work can also
  land locally.

## Checks

- `gh pr view <n> --json state,isDraft,url,headRefOid,statusCheckRollup,
  reviewDecision,mergeStateStatus`, per open PR.
- Polled while the project is open: every 20 s while checks run, for 10
  minutes, then every minute, then every 5; stops when all checks are done,
  and for good when the PR is merged or closed. At most 5 PRs per project
  are polled at once, the most recently active first. A Refresh button
  asks at once. All of this uses the person's own GitHub rate limit, so
  Ovra keeps it low.
- Each check links to its page (opened through `Opener`). A failed one
  offers **Send log to the worker**: `gh run view <run> --log-failed`,
  the last 200 lines and at most 32 KiB, sent as a comment framed as CI
  output, which is data and not instructions (like `read_output`).

## Shape

- New service trait `Forge` in `crates/core/src/services.rs`, with a fake
  in `fakes.rs` and the real one in a new crate `crates/forge`, wired in by
  `daemon` and `app`. It is the only place Ovra starts processes that use
  the person's GitHub credentials:
  `status(root, remote)`, `fetch(root, remote, branch)`,
  `push(root, remote, commit, branch, expect)`, `create(root, PrDraft)`,
  `view(root, number)`, `failed_log(root, run)`.
- The pick and commit are plain git without credentials:
  `Worktrees::pick(root, base, start, reviewed, message, author)` returns a
  commit or the conflicting files.
- Protocol domain `pr` (`protocol/pr.rs`, `ui/src/lib/protocol/pr.ts`):
  `pr.status`, `pr.prepare` (drafted title and body, base, branch, files,
  conflicts), `pr.create`, `pr.update`, `pr.refresh`, `pr.send_log`;
  event `pr.changed`. `ReviewItem` gains `pull_request`.
- Processes run with timeouts (fetch and push 2 minutes, `gh` calls 30 s)
  and can be cancelled from the dialog, so a hanging credential prompt or
  gpg never blocks the core. Assumption: there is no `pr.cancel` request;
  dropping the call cancels it (the client closing the dialog drops the
  request). A dropped or timed-out call kills `gh` or `git`, and on Unix
  its whole process group, so the `ssh` or credential helper git started
  ends with it; on Windows only `gh` or `git` itself is killed for now.
- Every `gh` call names the repository (`--repo HOST/OWNER/REPO`), taken
  from the chosen remote's URL when the pull request is opened and kept
  with it, so `gh` never picks another remote such as `upstream`. Fetch
  and push run without the repository's hooks, and a fetch reads the
  commit from a ref of its own, never `FETCH_HEAD`.

| Step | Area                                 | What                                                          |
| ---- | ------------------------------------ | ------------------------------------------------------------- |
| PR.0 | `services.rs`, protocol, crate graph | `Forge` trait and fake, `Worktrees::pick`, `pr.*`, stub crate |
| PR.A | `crates/forge`                       | `gh` and `git` processes, timeouts, URL scrubbing             |
| PR.B | `crates/worktree`                    | `pick` with `merge-tree` and `commit-tree`                    |
| PR.C | `crates/core/src/orchestration`      | `published` state, polling, send log, audit lines             |
| PR.D | `ui/src/inbox`                       | dialog, chip, checks list, Update, Send log                   |

`crates/forge` is security code (it handles the person's credentials) and
needs an independent reviewer agent's verdict.

## Tests

- Fakes (`MemoryForge`, `MemoryWorktrees`): prepare, create, update with a
  moved branch refused, polling backoff on a fake clock, signed-out and
  missing `gh` states, log size cap, audit lines.
- `crates/worktree` against real git: the pick drops the person's
  uncommitted work and keeps the worker's, conflicts are named.
- `crates/forge` against a fake `gh` script on `PATH` and a local bare
  repository as the remote: the exact arguments, body on stdin, no
  `--show-token`, a token in a remote URL never appears in errors.
- e2e on fakes: open the dialog, create, the chip shows checks.

## Decisions for the owner

1. **A new crate `crates/forge` and ADR 0010 with the credentials rule
   above.** Recommended: yes; it keeps every process with the person's
   GitHub login in one reviewed place.
2. **Push a fresh commit on the remote's base branch, not the worker's
   branch.** Recommended: yes; the worker's branch carries the person's
   uncommitted work.
3. **No hooks on the PR commit and `--no-verify` on push.** Recommended:
   yes; hooks would run the agent's code outside the sandbox, and CI runs
   the checks. The alternative is to run them, as the git panel does for
   the person's own commits.
4. **Draft PRs by default.** Recommended: yes; marking one ready is one
   click on GitHub.
5. **Only the person can push or open a PR; agents get no tool for it.**
   Recommended: yes for v1.
