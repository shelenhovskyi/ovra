# Live agent suite

Runs the real `claude` CLI through Ovra: the headless daemon with the real
agent runtime (no `--fake-agent`), driven through the UI by Playwright.

```
just agents-live          # check that the installed Claude Code still works with Ovra
just agents-live record   # also rewrite crates/agent-runtime/fixtures from the sessions
```

Opt-in only: it needs Claude Code installed and signed in, it uses the
plan's limits, and real turns take seconds each, so it is not part of
`just test`, `just e2e` or the time budgets. Run it after a Claude Code
update and before raising the supported version range. Linux and macOS.

- `global-setup.ts` starts the daemon with the environment a person's own
  shell would give it: variables of a Claude Code session the suite may run
  in (`CLAUDE*`, `CCR_*`) are removed, because they change what the CLI
  prints. The model is `haiku` unless `OVRA_LIVE_MODEL` says otherwise
  (`default` keeps the user's own).
- `recorder.ts` stands in for `claude` on the daemon's `PATH`: it runs the
  real CLI unchanged and writes each session's stdin and stdout lines to
  `test-results/recordings/`. Those files are raw and contain local paths and
  ids; they stay out of git.
- `fixtures.ts` turns a recording into a fixture: paths become `/w` (the
  project) and `/` (its parent), ids and times become placeholders, plan
  usage and the machine's commands, skills and plugins are dropped, and the
  core's steps (answers, replies, interrupts, stop) are added. It refuses to
  write a fixture that still has a home or temp path or a key.
- `specs/claude.spec.ts`: allow an edit and a command over two turns; a
  write outside the project that the project's settings allow but Ovra's
  `PreToolUse` hook turns into a prompt, denied with a message; interrupting
  a running command and going on; a subagent; resuming a session with
  Ovra's flags (the CLI directly: `agents.*` has no resume yet).
- `specs/task.spec.ts`: "Plan and split" starts a coordinator that starts two
  workers through Ovra's MCP tools; each writes one file in its worktree and
  asks for review. The test checks both review items and that the task cost
  less than $1 (about half a cent with haiku).
