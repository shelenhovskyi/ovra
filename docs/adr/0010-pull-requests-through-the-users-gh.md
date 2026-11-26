# 0010. Pull requests through the user's installed `gh`

- Status: accepted
- Date: 2026-11-25

## Context

People want a worker's reviewed result to become a pull request, with its
CI checks visible in Ovra (`docs/specs/pr-flow.md`). That needs pushes and
GitHub API calls with the person's GitHub account. Ovra could ship its own
OAuth app and keep tokens, or drive the GitHub CLI the person already
installed and signed in to, as ADR 0006 does for `claude`.

## Decision

- Ovra runs the person's installed `gh` and `git` unmodified, outside the
  sandbox, only after a person's click in a dialog that shows what is sent.
  It never bundles, downloads or installs `gh`, and has no GitHub sign-in
  UI.
- Ovra never reads, copies or passes on GitHub credentials (`gh`'s hosts
  file, the keychain, `GH_TOKEN`, `GITHUB_TOKEN`, git credential stores),
  never calls GitHub's API itself and never passes `--show-token`. The
  processes inherit the person's environment; `gh` and git find the login.
- Agents get no tool for opening pull requests, and their sandbox keeps
  hiding `gh`'s configuration and token variables. Amended 2026-11-26 on
  the owner's request (`docs/specs/agent-push.md`): a worker may ask Ovra
  to push its changes with the IDE tool `push`. Ovra builds the commit and
  runs the push as the person, after the person's Allow on an approval
  card or a `Push(<remote> <branch>)` rule the person added; never forced,
  never with protected files, and protected branches ask unless a rule
  names them exactly.
- A new crate, `crates/forge`, implements the `Forge` service trait. It is
  the only place that starts processes using the person's GitHub login;
  it is security code and needs an independent reviewer agent's verdict.
- The pull request's commit is built from the reviewed snapshot on top of
  the fetched base branch, as the person, without hooks, and pushed with
  `--no-verify` and never with `--force`, so the agent's code does not run
  outside the sandbox and the person's uncommitted work is not published.

## Consequences

- No Ovra GitHub app, client secret or token storage to protect.
- People without `gh` see how to install it and sign in; nothing else.
- Other forges (GitLab through `glab`) can implement `Forge` later.
- Checks are polled with `gh`, within the person's own rate limit, so
  polling backs off.
